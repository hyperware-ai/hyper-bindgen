use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use syn::{self, Attribute, ImplItem, Item, Type};
use walkdir::WalkDir;
use toml::Value;

// Helper functions for naming conventions
fn to_kebab_case(s: &str) -> String {
    // First, handle the case where the input has underscores
    if s.contains('_') {
        return s.replace('_', "-");
    }
    
    let mut result = String::with_capacity(s.len() + 5); // Extra capacity for hyphens
    let chars: Vec<char> = s.chars().collect();
    
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            // Add hyphen if:
            // 1. Not the first character
            // 2. Previous character is lowercase
            // 3. Or next character is lowercase (to handle acronyms like HTML)
            if i > 0 && 
               (chars[i-1].is_lowercase() ||
                (i < chars.len() - 1 && chars[i+1].is_lowercase()))
            {
                result.push('-');
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    
    result
}

// Validates a name doesn't contain numbers or "stream"
fn validate_name(name: &str, kind: &str) -> Result<()> {
    // Check for numbers
    if name.chars().any(|c| c.is_digit(10)) {
        anyhow::bail!("Error: {} name '{}' contains numbers, which is not allowed", kind, name);
    }
    
    // Check for "stream"
    if name.to_lowercase().contains("stream") {
        anyhow::bail!("Error: {} name '{}' contains 'stream', which is not allowed", kind, name);
    }
    
    Ok(())
}

// Remove "State" suffix from a name
fn remove_state_suffix(name: &str) -> String {
    if name.ends_with("State") {
        let len = name.len();
        return name[0..len-5].to_string();
    }
    name.to_string()
}

// Extract wit_world from the #[hyperprocess] attribute using the format in the debug representation
fn extract_wit_world(attrs: &[Attribute]) -> Result<String> {
    for attr in attrs {
        if attr.path().is_ident("hyperprocess") {
            // Convert attribute to string representation
            let attr_str = format!("{:?}", attr);
            println!("Attribute string: {}", attr_str);
            
            // Look for wit_world in the attribute string
            if let Some(pos) = attr_str.find("wit_world") {
                println!("Found wit_world at position {}", pos);
                
                // Find the literal value after wit_world by looking for lit: "value"
                let lit_pattern = "lit: \"";
                if let Some(lit_pos) = attr_str[pos..].find(lit_pattern) {
                    let start_pos = pos + lit_pos + lit_pattern.len();
                    
                    // Find the closing quote of the literal
                    if let Some(quote_pos) = attr_str[start_pos..].find('\"') {
                        let world_name = &attr_str[start_pos..(start_pos + quote_pos)];
                        println!("Extracted wit_world: {}", world_name);
                        return Ok(world_name.to_string());
                    }
                }
            }
        }
    }
    anyhow::bail!("wit_world not found in hyperprocess attribute")
}

// Convert Rust type to WIT type, including downstream types
fn rust_type_to_wit(ty: &Type, used_types: &mut HashSet<String>) -> Result<String> {
    match ty {
        Type::Path(type_path) => {
            if type_path.path.segments.is_empty() {
                return Ok("unknown".to_string());
            }
            
            let ident = &type_path.path.segments.last().unwrap().ident;
            let type_name = ident.to_string();
            
            match type_name.as_str() {
                "i32" => Ok("s32".to_string()),
                "u32" => Ok("u32".to_string()),
                "i64" => Ok("s64".to_string()),
                "u64" => Ok("u64".to_string()),
                "f32" => Ok("f32".to_string()),
                "f64" => Ok("f64".to_string()),
                "String" => Ok("string".to_string()),
                "bool" => Ok("bool".to_string()),
                "Vec" => {
                    if let syn::PathArguments::AngleBracketed(args) = 
                        &type_path.path.segments.last().unwrap().arguments
                    {
                        if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                            let inner_type = rust_type_to_wit(inner_ty, used_types)?;
                            Ok(format!("list<{}>", inner_type))
                        } else {
                            Ok("list<any>".to_string())
                        }
                    } else {
                        Ok("list<any>".to_string())
                    }
                }
                "Option" => {
                    if let syn::PathArguments::AngleBracketed(args) =
                        &type_path.path.segments.last().unwrap().arguments
                    {
                        if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                            let inner_type = rust_type_to_wit(inner_ty, used_types)?;
                            Ok(format!("option<{}>", inner_type))
                        } else {
                            Ok("option<any>".to_string())
                        }
                    } else {
                        Ok("option<any>".to_string())
                    }
                }
                custom => {
                    // Validate custom type name
                    validate_name(custom, "Type")?;
                    
                    // Convert custom type to kebab-case and add to used types
                    let kebab_custom = to_kebab_case(custom);
                    used_types.insert(kebab_custom.clone());
                    Ok(kebab_custom)
                }
            }
        }
        Type::Reference(type_ref) => {
            // Handle references by using the underlying type
            rust_type_to_wit(&type_ref.elem, used_types)
        }
        Type::Tuple(type_tuple) => {
            if type_tuple.elems.is_empty() {
                // Empty tuple is unit in WIT
                Ok("unit".to_string())
            } else {
                // Create a tuple representation in WIT
                let mut elem_types = Vec::new();
                for elem in &type_tuple.elems {
                    elem_types.push(rust_type_to_wit(elem, used_types)?);
                }
                Ok(format!("tuple<{}>", elem_types.join(", ")))
            }
        }
        _ => Ok("unknown".to_string()),
    }
}

// Collect type definitions (structs and enums) from the file
fn collect_type_definitions(ast: &syn::File) -> Result<HashMap<String, String>> {
    let mut type_defs = HashMap::new();
    
    println!("Collecting type definitions from file");
    for item in &ast.items {
        match item {
            Item::Struct(item_struct) => {
                // Validate struct name doesn't contain numbers or "stream"
                let orig_name = item_struct.ident.to_string();
                validate_name(&orig_name, "Struct")?;
                
                // Use kebab-case for struct name
                let name = to_kebab_case(&orig_name);
                println!("  Found struct: {}", name);
                
                let fields: Vec<String> = match &item_struct.fields {
                    syn::Fields::Named(fields) => {
                        let mut used_types = HashSet::new();
                        let mut field_strings = Vec::new();
                        
                        for f in &fields.named {
                            if let Some(field_ident) = &f.ident {
                                // Validate field name doesn't contain digits
                                let field_orig_name = field_ident.to_string();
                                validate_name(&field_orig_name, "Field")?;
                                
                                // Convert field names to kebab-case
                                let field_name = to_kebab_case(&field_orig_name);
                                let field_type = rust_type_to_wit(&f.ty, &mut used_types)?;
                                println!("    Field: {} -> {}", field_name, field_type);
                                field_strings.push(format!("        {}: {}", field_name, field_type));
                            }
                        }
                        
                        field_strings
                    }
                    _ => Vec::new(),
                };
                
                if !fields.is_empty() {
                    type_defs.insert(
                        name.clone(),
                        format!("    record {} {{\n{}\n    }}", name, fields.join(",\n")), // Add comma separator
                    );
                }
            }
            Item::Enum(item_enum) => {
                // Validate enum name doesn't contain numbers or "stream"
                let orig_name = item_enum.ident.to_string();
                validate_name(&orig_name, "Enum")?;
                
                // Use kebab-case for enum name
                let name = to_kebab_case(&orig_name);
                println!("  Found enum: {}", name);
                
                let variants: Vec<String> = item_enum
                    .variants
                    .iter()
                    .map(|v| {
                        let variant_orig_name = v.ident.to_string();
                        // Validate variant name
                        validate_name(&variant_orig_name, "Enum variant")?;
                        
                        match &v.fields {
                            syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                                let mut used_types = HashSet::new();
                                let ty = rust_type_to_wit(
                                    &fields.unnamed.first().unwrap().ty,
                                    &mut used_types
                                )?;
                                
                                // Use kebab-case for variant names and use parentheses for type
                                let variant_name = to_kebab_case(&variant_orig_name);
                                println!("    Variant: {} -> {}", variant_name, ty);
                                Ok(format!("        {}({})", variant_name, ty))
                            }
                            syn::Fields::Unit => {
                                // Use kebab-case for variant names
                                let variant_name = to_kebab_case(&variant_orig_name);
                                println!("    Variant: {}", variant_name);
                                Ok(format!("        {}", variant_name))
                            },
                            _ => {
                                // Use kebab-case for variant names
                                let variant_name = to_kebab_case(&variant_orig_name);
                                println!("    Variant: {} (complex)", variant_name);
                                Ok(format!("        {}", variant_name))
                            },
                        }
                    })
                    .collect::<Result<Vec<String>>>()?;
                
                type_defs.insert(
                    name.clone(),
                    format!("    variant {} {{\n{}\n    }}", name, variants.join(",\n")), // Add comma separator
                );
            }
            _ => {}
        }
    }
    
    println!("Collected {} type definitions", type_defs.len());
    Ok(type_defs)
}

// Find all relevant Rust projects
fn find_rust_projects(base_dir: &Path) -> Vec<PathBuf> {
    let mut projects = Vec::new();
    println!("Scanning for Rust projects in {}", base_dir.display());
    
    for entry in WalkDir::new(base_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        
        if path.is_dir() && path != base_dir {
            let cargo_toml = path.join("Cargo.toml");
            println!("Checking {}", cargo_toml.display());
            
            if cargo_toml.exists() {
                // Try to read and parse Cargo.toml
                if let Ok(content) = fs::read_to_string(&cargo_toml) {
                    if let Ok(cargo_data) = content.parse::<Value>() {
                        // Check for the specific metadata
                        if let Some(metadata) = cargo_data
                            .get("package")
                            .and_then(|p| p.get("metadata"))
                            .and_then(|m| m.get("component"))
                        {
                            if let Some(package) = metadata.get("package") {
                                if let Some(package_str) = package.as_str() {
                                    println!("  Found package.metadata.component.package = {:?}", package_str);
                                    if package_str == "hyperware:process" {
                                        println!("  Adding project: {}", path.display());
                                        projects.push(path.to_path_buf());
                                    }
                                }
                            }
                        } else {
                            println!("  No package.metadata.component metadata found");
                        }
                    }
                }
            }
        }
    }
    
    println!("Found {} relevant Rust projects", projects.len());
    projects
}

// Helper function to generate signature struct for specific attribute type
fn generate_signature_struct(
    kebab_name: &str,
    attr_type: &str,
    method: &syn::ImplItemFn,
    used_types: &mut HashSet<String>,
) -> Result<String> {
    // Create signature struct name with attribute type
    let signature_struct_name = format!("{}-signature-{}", kebab_name, attr_type);
    
    // Generate comment for this specific function
    let comment = format!("    // Function signature for: {} ({})", kebab_name, attr_type);
    
    // Create struct fields that directly represent function parameters
    let mut struct_fields = Vec::new();
    
    // Add target parameter based on attribute type
    if attr_type == "http" {
        struct_fields.push("        target: string".to_string());
    } else { // remote or local
        struct_fields.push("        target: address".to_string());
    }
    
    // Process function parameters (skip &self and &mut self)
    for arg in &method.sig.inputs {
        if let syn::FnArg::Typed(pat_type) = arg {
            if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                // Skip &self and &mut self
                if pat_ident.ident == "self" {
                    continue;
                }
                
                // Get original param name and convert to kebab-case
                let param_orig_name = pat_ident.ident.to_string();
                
                // Validate parameter name
                validate_name(&param_orig_name, "Parameter")?;
                
                let param_name = to_kebab_case(&param_orig_name);
                
                // Rust type to WIT type
                let param_type = rust_type_to_wit(&pat_type.ty, used_types)?;
                
                // Add field directly to the struct
                struct_fields.push(format!("        {}: {}", param_name, param_type));
            }
        }
    }
    
    // Add return type field
    match &method.sig.output {
        syn::ReturnType::Type(_, ty) => {
            let return_type = rust_type_to_wit(&*ty, used_types)?;
            struct_fields.push(format!("        returning: {}", return_type));
        }
        _ => {
            // For unit return type
            struct_fields.push("        returning: unit".to_string());
        }
    }
    
    // Combine everything into a record definition
    let record_def = format!(
        "{}\n    record {} {{\n{}\n    }}",
        comment,
        signature_struct_name,
        struct_fields.join(",\n")
    );
    
    Ok(record_def)
}

// Generate WIT content for an interface
fn generate_interface_wit_content(
    impl_item: &syn::ItemImpl,
    interface_name: &str,
    ast: &syn::File,
) -> Result<String> {
    let mut signature_structs = Vec::new();
    let mut used_types = HashSet::new();
    
    // Extract the base name without "State" suffix for the interface
    let base_name = remove_state_suffix(interface_name);
    
    // Convert interface name to kebab-case for the interface declaration
    let kebab_interface_name = to_kebab_case(&base_name);
    println!("Generating WIT content for interface: {} (kebab: {})", interface_name, kebab_interface_name);
    
    // We'll use a simpler comment approach now, added directly in the content generation code
    
    for item in &impl_item.items {
        if let ImplItem::Fn(method) = item {
            let method_name = method.sig.ident.to_string();
            println!("  Examining method: {}", method_name);
            
            // Check for attribute types
            let has_remote = method.attrs.iter().any(|attr| attr.path().is_ident("remote"));
            let has_local = method.attrs.iter().any(|attr| attr.path().is_ident("local"));
            let has_http = method.attrs.iter().any(|attr| attr.path().is_ident("http"));
            
            if has_remote || has_local || has_http {
                println!("    Has relevant attributes: remote={}, local={}, http={}", 
                    has_remote, has_local, has_http);
                
                // Validate function name
                validate_name(&method_name, "Function")?;
                
                // Convert function name to kebab-case
                let kebab_name = to_kebab_case(&method_name);
                println!("    Processing method: {} -> {}", method_name, kebab_name);
                
                // Generate a signature struct for each attribute type
                if has_remote {
                    let remote_struct = generate_signature_struct(
                        &kebab_name, "remote", method, &mut used_types
                    )?;
                    signature_structs.push(remote_struct);
                }
                
                if has_local {
                    let local_struct = generate_signature_struct(
                        &kebab_name, "local", method, &mut used_types
                    )?;
                    signature_structs.push(local_struct);
                }
                
                if has_http {
                    let http_struct = generate_signature_struct(
                        &kebab_name, "http", method, &mut used_types
                    )?;
                    signature_structs.push(http_struct);
                }
            } else {
                println!("    Skipping method without relevant attributes");
            }
        }
    }
    
    // Collect all type definitions from the file
    let all_type_defs = collect_type_definitions(ast)?;
    
    // Filter for only the types we're using
    let mut type_defs = Vec::new();
    let mut processed_types = HashSet::new();
    let mut types_to_process: Vec<String> = used_types.into_iter().collect();
    
    println!("Processing used types: {:?}", types_to_process);
    
    // Process all referenced types and their dependencies
    while let Some(type_name) = types_to_process.pop() {
        if processed_types.contains(&type_name) {
            continue;
        }
        
        processed_types.insert(type_name.clone());
        println!("  Processing type: {}", type_name);
        
        if let Some(type_def) = all_type_defs.get(&type_name) {
            println!("    Found type definition");
            type_defs.push(type_def.clone());
            
            // Extract any types referenced in this type definition
            for referenced_type in all_type_defs.keys() {
                if type_def.contains(referenced_type) && !processed_types.contains(referenced_type) {
                    println!("    Adding referenced type: {}", referenced_type);
                    types_to_process.push(referenced_type.clone());
                }
            }
        } else {
            println!("    No definition found for type: {}", type_name);
        }
    }
    
    // Generate the final WIT content
    if signature_structs.is_empty() {
        println!("No functions found for interface {}", interface_name);
        Ok(String::new())
    } else {
        // Start with a simplified interface comment
        let mut content = "    // This interface contains function signature definitions that will be used\n    // by the hyper-bindgen macro to generate async function bindings.\n    //\n    // NOTE: This is currently a hacky workaround since WIT async functions are not\n    // available until WASI Preview 3. Once Preview 3 is integrated into Hyperware,\n    // we should switch to using proper async WIT function signatures instead of\n    // this struct-based approach with hyper-bindgen generating the async stubs.\n".to_string();
        
        // Add standard imports
        content.push_str("\n    use standard.{address};\n\n");
        
        // Add type definitions if any
        if !type_defs.is_empty() {
            content.push_str(&type_defs.join("\n\n"));
            content.push_str("\n\n");
        }
        
        // Add signature structs
        content.push_str(&signature_structs.join("\n\n"));
        
        // Wrap in interface block
        let final_content = format!("interface {} {{\n{}\n}}\n", kebab_interface_name, content);
        println!("Generated interface content for {} with {} signature structs", interface_name, signature_structs.len());
        Ok(final_content)
    }
}

// Helper trait to get TypePath from Type
trait AsTypePath {
    fn as_type_path(&self) -> Option<&syn::TypePath>;
}

impl AsTypePath for syn::Type {
    fn as_type_path(&self) -> Option<&syn::TypePath> {
        match self {
            syn::Type::Path(tp) => Some(tp),
            _ => None,
        }
    }
}

// Process a single Rust project and return the generated WIT content
fn process_rust_project(project_path: &Path) -> Result<Option<(String, String, String, String)>> {
    println!("\nProcessing project: {}", project_path.display());
    let lib_rs = project_path.join("src").join("lib.rs");
    
    println!("Looking for lib.rs at {}", lib_rs.display());
    if !lib_rs.exists() {
        println!("No lib.rs found for project: {}", project_path.display());
        return Ok(None);
    }
    
    let lib_content = fs::read_to_string(&lib_rs)
        .with_context(|| format!("Failed to read lib.rs for project: {}", project_path.display()))?;
    
    println!("Successfully read lib.rs, parsing...");
    let ast = syn::parse_file(&lib_content)
        .with_context(|| format!("Failed to parse lib.rs for project: {}", project_path.display()))?;
    
    println!("Successfully parsed lib.rs");
    
    let mut wit_world = None;
    let mut interface_name = None;
    let mut kebab_interface_name = None;
    let mut interface_content = None;
    
    println!("Scanning for impl blocks with hyperprocess attribute");
    for item in &ast.items {
        if let Item::Impl(impl_item) = item {
            println!("Found impl block");
            
            // Check if this impl block has a #[hyperprocess] attribute
            if let Some(attr) = impl_item.attrs.iter().find(|attr| attr.path().is_ident("hyperprocess")) {
                println!("Found hyperprocess attribute");
                
                // Extract the wit_world name
                match extract_wit_world(&[attr.clone()]) {
                    Ok(world_name) => {
                        println!("Extracted wit_world: {}", world_name);
                        wit_world = Some(world_name);
                        
                        // Get the interface name from the impl type
                        interface_name = impl_item
                            .self_ty
                            .as_ref()
                            .as_type_path()
                            .map(|tp| {
                                if let Some(last_segment) = tp.path.segments.last() {
                                    last_segment.ident.to_string()
                                } else {
                                    "Unknown".to_string()
                                }
                            });
                        
                        // Check for "State" suffix and remove it
                        if let Some(ref name) = interface_name {
                            // Validate the interface name
                            validate_name(name, "Interface")?;
                            
                            // Remove State suffix if present
                            let base_name = remove_state_suffix(name);
                            
                            // Convert to kebab-case for file name and interface name
                            kebab_interface_name = Some(to_kebab_case(&base_name));
                            
                            println!("Interface name: {:?}", interface_name);
                            println!("Base name: {}", base_name);
                            println!("Kebab interface name: {:?}", kebab_interface_name);
                        }
                        
                        if let (Some(ref iface_name), Some(ref kebab_name)) = (&interface_name, &kebab_interface_name) {
                            // We already validated the interface name, so the file name should be fine
                            
                            // Generate the WIT content
                            let content = generate_interface_wit_content(impl_item, iface_name, &ast)?;
                            
                            if !content.is_empty() {
                                interface_content = Some(content);
                                println!("Generated WIT content for interface {}", kebab_name);
                            } else {
                                println!("Generated WIT content is empty, skipping");
                            }
                        }
                    },
                    Err(e) => println!("Failed to extract wit_world: {}", e),
                }
            }
        }
    }
    
    if let (Some(world), Some(_), Some(kebab_iface), Some(content)) = 
           (&wit_world, &interface_name, &kebab_interface_name, &interface_content) {
        println!("Returning import statement for interface {} in world {}", kebab_iface, world);
        // Return the import statement, world name, interface name, and content
        Ok(Some((
            format!("    import {};", kebab_iface),
            world.clone(),
            kebab_iface.clone(),
            content.clone()
        )))
    } else {
        println!("No valid interface found");
        Ok(None)
    }
}

// Delete all WIT files in the API directory
fn clean_api_directory(api_dir: &Path) -> Result<()> {
    println!("Cleaning API directory: {}", api_dir.display());
    
    // Make sure the directory exists
    if !api_dir.exists() {
        fs::create_dir_all(api_dir)
            .with_context(|| format!("Failed to create API directory: {}", api_dir.display()))?;
        return Ok(());
    }
    
    // Find and delete all .wit files
    let mut deleted_count = 0;
    for entry in WalkDir::new(api_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        
        if path.is_file() && path.extension().map_or(false, |ext| ext == "wit") {
            println!("Deleting WIT file: {}", path.display());
            fs::remove_file(path)
                .with_context(|| format!("Failed to delete WIT file: {}", path.display()))?;
            deleted_count += 1;
        }
    }
    
    println!("Deleted {} WIT files from {}", deleted_count, api_dir.display());
    Ok(())
}

// Find all API directories recursively within the test folder
fn find_test_api_directories(base_dir: &Path) -> Vec<PathBuf> {
    let mut api_dirs = Vec::new();
    let test_dir = base_dir.join("test");
    
    if !test_dir.exists() || !test_dir.is_dir() {
        println!("No test folder found in {}", base_dir.display());
        return api_dirs;
    }
    
    println!("Recursively scanning for API directories within test folder: {}", test_dir.display());
    
    for entry in WalkDir::new(test_dir)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        
        if path.is_dir() && path.file_name().map_or(false, |name| name == "api") {
            println!("Found API directory within test folder: {}", path.display());
            api_dirs.push(path.to_path_buf());
        }
    }
    
    if api_dirs.is_empty() {
        println!("No API directories found within test folder");
    } else {
        println!("Found {} API directories within test folder", api_dirs.len());
    }
    
    api_dirs
}

// Write interface files to an API directory
fn write_interface_files(
    api_dir: &Path,
    interface_contents: &HashMap<String, String>,
    world_names: &HashSet<String>,
    world_imports: &Vec<String>,
    is_test: bool, // Flag to indicate if this is for a test API directory
) -> Result<()> {
    println!("Writing interface files to {}", api_dir.display());
    
    // Make sure the API directory exists
    if !api_dir.exists() {
        fs::create_dir_all(api_dir)
            .with_context(|| format!("Failed to create API directory: {}", api_dir.display()))?;
    }
    
    // Write all interface files
    for (interface_name, content) in interface_contents {
        let interface_file = api_dir.join(format!("{}.wit", interface_name));
        println!("Writing interface file {} to {}", interface_name, interface_file.display());
        
        fs::write(&interface_file, content)
            .with_context(|| format!("Failed to write interface file: {}", interface_file.display()))?;
        
        println!("Successfully wrote interface file: {}", interface_name);
    }
    
    // Look for existing world definition files and update them
    let mut found_existing_worlds = HashSet::new();
    
    println!("Looking for existing world definition files in {}", api_dir.display());
    for entry in WalkDir::new(api_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        
        if path.is_file() && path.extension().map_or(false, |ext| ext == "wit") {
            println!("Checking WIT file: {}", path.display());
            
            if let Ok(content) = fs::read_to_string(path) {
                if content.contains("world ") {
                    println!("Found world definition file");
                    
                    // Extract the world name
                    let lines: Vec<&str> = content.lines().collect();
                    
                    if let Some(world_line) = lines.iter().find(|line| line.trim().starts_with("world ")) {
                        println!("World line: {}", world_line);
                        
                        if let Some(world_name) = world_line.trim().split_whitespace().nth(1) {
                            let clean_name = world_name.trim_end_matches(" {");
                            println!("Extracted world name: {}", clean_name);
                            
                            // If this is one of our collected world names, update it
                            if world_names.contains(clean_name) {
                                found_existing_worlds.insert(clean_name.to_string());
                                
                                // Create updated world content with process-v1 include
                                let world_content = if is_test {
                                    // For test API, add prefix to world name and import tester line
                                    let test_world_name = format!("test-{}", clean_name);
                                    format!(
                                        "world {} {{\n    import tester;\n{}\n    include process-v1;\n}}",
                                        test_world_name,
                                        world_imports.join("\n")
                                    )
                                } else {
                                    // Standard world content
                                    format!(
                                        "world {} {{\n{}\n    include process-v1;\n}}",
                                        clean_name,
                                        world_imports.join("\n")
                                    )
                                };
                                
                                if is_test {
                                    // For test API, write to a new file with test- prefix
                                    let test_world_name = format!("test-{}", clean_name);
                                    let test_world_file = api_dir.join(format!("{}.wit", test_world_name));
                                    println!("Writing test world definition to {}", test_world_file.display());
                                    fs::write(test_world_file.clone(), world_content)
                                        .with_context(|| format!("Failed to write test world file: {}", test_world_file.display()))?;
                                } else {
                                    // Standard API, update existing file
                                    println!("Writing updated world definition to {}", path.display());
                                    fs::write(path, world_content)
                                        .with_context(|| format!("Failed to write updated world file: {}", path.display()))?;
                                }
                                
                                println!("Successfully updated world definition");
                                
                                // Also create or update the types world
                                let types_world_name = if is_test {
                                    format!("types-test-{}", clean_name)
                                } else {
                                    format!("types-{}", clean_name)
                                };
                                let types_world_file = api_dir.join(format!("{}.wit", types_world_name));
                                
                                // Create types world content with lib include
                                let world_name_in_content = if is_test {
                                    format!("types-test-{}", clean_name)
                                } else {
                                    format!("types-{}", clean_name)
                                };
                                let types_world_content = format!(
                                    "world {} {{\n{}\n    include lib;\n}}",
                                    world_name_in_content,
                                    world_imports.join("\n")
                                );
                                
                                println!("Writing types world definition to {}", types_world_file.display());
                                fs::write(types_world_file.clone(), types_world_content)
                                    .with_context(|| format!("Failed to write types world file: {}", types_world_file.display()))?;
                                
                                println!("Successfully created/updated types world definition");
                            }
                        }
                    }
                }
            }
        }
    }
    
    // For any world names that weren't found in existing files, create new ones
    let missing_worlds: Vec<&String> = world_names.iter()
        .filter(|name| !found_existing_worlds.contains(*name))
        .collect();
    
    for world_name in missing_worlds {
        println!("Creating new world definition for: {}", world_name);
        
        // Create world content with process-v1 include
        let (world_file_name, world_declaration_name) = if is_test {
            // For test API, add test- prefix to both file name and world declaration
            let test_name = format!("test-{}", world_name);
            (test_name.clone(), test_name)
        } else {
            // Standard API, use world_name as is
            (world_name.to_string(), world_name.to_string())
        };
        
        let world_content = if is_test {
            // For test API, add import tester line
            format!(
                "world {} {{\n    import tester;\n{}\n    include process-v1;\n}}",
                world_declaration_name,
                world_imports.join("\n")
            )
        } else {
            // Standard world content
            format!(
                "world {} {{\n{}\n    include process-v1;\n}}",
                world_declaration_name,
                world_imports.join("\n")
            )
        };
        
        let world_file = api_dir.join(format!("{}.wit", world_file_name));
        println!("Writing world definition to {}", world_file.display());
        
        fs::write(world_file.clone(), world_content)
            .with_context(|| format!("Failed to write world file: {}", world_file.display()))?;
        
        println!("Successfully created world definition");
        
        // Also create the types world
        let types_world_name = if is_test {
            format!("types-test-{}", world_name)
        } else {
            format!("types-{}", world_name)
        };
        let types_world_file = api_dir.join(format!("{}.wit", types_world_name));
        
        // Create types world content with lib include
        let types_world_content = format!(
            "world {} {{\n{}\n    include lib;\n}}",
            types_world_name,
            world_imports.join("\n")
        );
        
        println!("Writing types world definition to {}", types_world_file.display());
        fs::write(types_world_file.clone(), types_world_content)
            .with_context(|| format!("Failed to write types world file: {}", types_world_file.display()))?;
        
        println!("Successfully created types world definition");
    }
    
    Ok(())
}

// Generate WIT files from Rust code for top-level API directory and test/api directories if they exist
pub fn generate_wit_files(base_dir: &Path, top_level_api_dir: &Path) -> Result<(Vec<PathBuf>, Vec<String>)> {
    // Clean only the top-level API directory
    clean_api_directory(top_level_api_dir)?;
    
    // Find all relevant Rust projects
    let projects = find_rust_projects(base_dir);
    let mut processed_projects = Vec::new();
    
    if projects.is_empty() {
        println!("No relevant Rust projects found.");
        return Ok((Vec::new(), Vec::new()));
    }
    
    // Process each project and collect world imports, world names, and interface content
    let mut world_imports = Vec::new();
    let mut world_names = HashSet::new();
    let mut interfaces = Vec::new();
    let mut interface_contents = HashMap::new(); // Map of interface name to its content
    
    for project_path in &projects {
        println!("Processing project: {}", project_path.display());
        
        match process_rust_project(project_path) {
            Ok(Some((import, world_name, interface_name, content))) => {
                println!("Got import statement: {} for world: {}", import, world_name);
                world_imports.push(import.clone());
                world_names.insert(world_name);
                interfaces.push(interface_name.clone());
                interface_contents.insert(interface_name, content);
                processed_projects.push(project_path.clone());
            },
            Ok(None) => println!("No import statement generated"),
            Err(e) => println!("Error processing project: {}", e),
        }
    }
    
    println!("Collected {} world imports and {} interface contents", world_imports.len(), interface_contents.len());
    
    // Now we have world names from hyperprocess attributes, convert to HashSet for uniqueness
    if world_names.is_empty() && !world_imports.is_empty() {
        // If no world names were found, use default
        world_names.insert("async-app-template-dot-os-v0".to_string());
    }
    
    println!("Found world names: {:?}", world_names);
    
    // First, write interface files to the top-level API directory
    write_interface_files(top_level_api_dir, &interface_contents, &world_names, &world_imports, false)?;
    
    // Find and process all API directories within test folder
    let test_api_dirs = find_test_api_directories(base_dir);
    let has_test_dirs = !test_api_dirs.is_empty();
    
    // Check if any test API directories were found
    if has_test_dirs {
        // Process each found test API directory
        for test_api_dir in &test_api_dirs {
            println!("Processing test API directory: {}", test_api_dir.display());
            clean_api_directory(test_api_dir)?;
            write_interface_files(test_api_dir, &interface_contents, &world_names, &world_imports, true)?;
        }
    } else {
        println!("No API directories found within test folder, skipping test-specific WIT generation");
    }
    
    println!("WIT files generated successfully.");
    Ok((processed_projects, interfaces))
}