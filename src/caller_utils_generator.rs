use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value;
use walkdir::WalkDir;

// Convert kebab-case to snake_case
pub fn to_snake_case(s: &str) -> String {
    s.replace('-', "_")
}

// Convert kebab-case to PascalCase
pub fn to_pascal_case(s: &str) -> String {
    let parts = s.split('-');
    let mut result = String::new();
    
    for part in parts {
        if !part.is_empty() {
            let mut chars = part.chars();
            if let Some(first_char) = chars.next() {
                result.push(first_char.to_uppercase().next().unwrap());
                result.extend(chars);
            }
        }
    }
    
    result
}

// Find the world name in the world WIT file, prioritizing types-prefixed worlds
fn find_world_name(api_dir: &Path) -> Result<String> {
    let mut regular_world_name = None;
    let mut types_world_name = None;
    
    // Look for world definition files
    for entry in WalkDir::new(api_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        
        if path.is_file() && path.extension().map_or(false, |ext| ext == "wit") {
            if let Ok(content) = fs::read_to_string(path) {
                if content.contains("world ") {
                    println!("Analyzing world definition file: {}", path.display());
                    
                    // Extract the world name
                    let lines: Vec<&str> = content.lines().collect();
                    
                    if let Some(world_line) = lines.iter().find(|line| line.trim().starts_with("world ")) {
                        println!("World line: {}", world_line);
                        
                        if let Some(world_name) = world_line.trim().split_whitespace().nth(1) {
                            let clean_name = world_name.trim_end_matches(" {");
                            println!("Extracted world name: {}", clean_name);
                            
                            // Check if this is a types-prefixed world
                            if clean_name.starts_with("types-") {
                                types_world_name = Some(clean_name.to_string());
                                println!("Found types world: {}", clean_name);
                            } else {
                                regular_world_name = Some(clean_name.to_string());
                                println!("Found regular world: {}", clean_name);
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Prioritize types-prefixed world if found
    if let Some(types_name) = types_world_name {
        return Ok(types_name);
    }
    
    // If no types-prefixed world found, check if we have a regular world
    if let Some(regular_name) = regular_world_name {
        // Check if there's a corresponding types-prefixed world file
        let types_name = format!("types-{}", regular_name);
        let types_file = api_dir.join(format!("{}.wit", types_name));
        
        if types_file.exists() {
            println!("Found types world from file: {}", types_name);
            return Ok(types_name);
        }
        
        // Fall back to regular world but print a warning
        println!("Warning: No types- world found, using regular world: {}", regular_name);
        return Ok(regular_name);
    }
    
    // If no world name is found, we should fail
    bail!("No world name found in any WIT file. Cannot generate caller-utils without a world name.")
}

// Convert WIT type to Rust type - IMPROVED with more Rust primitives
fn wit_type_to_rust(wit_type: &str) -> String {
    match wit_type {
        // Integer types
        "s8" => "i8".to_string(),
        "u8" => "u8".to_string(),
        "s16" => "i16".to_string(),
        "u16" => "u16".to_string(),
        "s32" => "i32".to_string(),
        "u32" => "u32".to_string(),
        "s64" => "i64".to_string(),
        "u64" => "u64".to_string(),
        // Size types
        "usize" => "usize".to_string(),
        "isize" => "isize".to_string(),
        // Floating point types
        "f32" => "f32".to_string(),
        "f64" => "f64".to_string(),
        // Other primitive types
        "string" => "String".to_string(),
        "str" => "&str".to_string(),
        "char" => "char".to_string(),
        "bool" => "bool".to_string(),
        "unit" => "()".to_string(),
        // Special types
        "address" => "WitAddress".to_string(),
        // Common primitives that might be written differently in WIT
        "i8" => "i8".to_string(),
        "i16" => "i16".to_string(),
        "i32" => "i32".to_string(),
        "i64" => "i64".to_string(),
        // Collection types with generics
        t if t.starts_with("list<") => {
            let inner_type = &t[5..t.len() - 1];
            format!("Vec<{}>", wit_type_to_rust(inner_type))
        },
        t if t.starts_with("option<") => {
            let inner_type = &t[7..t.len() - 1];
            format!("Option<{}>", wit_type_to_rust(inner_type))
        },
        t if t.starts_with("result<") => {
            let inner_part = &t[7..t.len() - 1];
            if let Some(comma_pos) = inner_part.find(',') {
                let ok_type = &inner_part[..comma_pos].trim();
                let err_type = &inner_part[comma_pos + 1..].trim();
                format!("Result<{}, {}>", wit_type_to_rust(ok_type), wit_type_to_rust(err_type))
            } else {
                format!("Result<{}, ()>", wit_type_to_rust(inner_part))
            }
        },
        t if t.starts_with("tuple<") => {
            let inner_types = &t[6..t.len() - 1];
            let rust_types: Vec<String> = inner_types
                .split(", ")
                .map(|t| wit_type_to_rust(t))
                .collect();
            format!("({})", rust_types.join(", "))
        },
        // Handle map type if present
        t if t.starts_with("map<") => {
            let inner_part = &t[4..t.len() - 1];
            if let Some(comma_pos) = inner_part.find(',') {
                let key_type = &inner_part[..comma_pos].trim();
                let value_type = &inner_part[comma_pos + 1..].trim();
                format!("HashMap<{}, {}>", wit_type_to_rust(key_type), wit_type_to_rust(value_type))
            } else {
                // Fallback for malformed map type
                format!("HashMap<String, {}>", wit_type_to_rust(inner_part))
            }
        },
        // Custom types (in kebab-case) need to be converted to PascalCase
        _ => to_pascal_case(wit_type).to_string(),
    }
}

// Generate default value for Rust type - IMPROVED with additional types
fn generate_default_value(rust_type: &str) -> String {
    match rust_type {
        // Integer types
        "i8" | "u8" | "i16" | "u16" | "i32" | "u32" | "i64" | "u64" | "isize" | "usize" => "0".to_string(),
        // Floating point types
        "f32" | "f64" => "0.0".to_string(),
        // String types
        "String" => "String::new()".to_string(),
        "&str" => "\"\"".to_string(),
        // Other primitive types
        "bool" => "false".to_string(),
        "char" => "'\\0'".to_string(),
        "()" => "()".to_string(),
        // Collection types
        t if t.starts_with("Vec<") => "Vec::new()".to_string(),
        t if t.starts_with("Option<") => "None".to_string(),
        t if t.starts_with("Result<") => {
            // For Result, default to Ok with the default value of the success type
            if let Some(success_type_end) = t.find(',') {
                let success_type = &t[7..success_type_end];
                format!("Ok({})", generate_default_value(success_type))
            } else {
                "Ok(())".to_string()
            }
        },
        t if t.starts_with("HashMap<") => "HashMap::new()".to_string(),
        t if t.starts_with("(") => {
            // Generate default tuple with default values for each element
            let inner_part = t.trim_start_matches('(').trim_end_matches(')');
            let parts: Vec<_> = inner_part.split(", ").collect();
            let default_values: Vec<_> = parts.iter()
                .map(|part| generate_default_value(part))
                .collect();
            format!("({})", default_values.join(", "))
        },
        // For custom types, assume they implement Default
        _ => format!("{}::default()", rust_type),
    }
}

// Structure to represent a field in a WIT signature struct
struct SignatureField {
    name: String,
    wit_type: String,
}

// Structure to represent a WIT signature struct
struct SignatureStruct {
    function_name: String,
    attr_type: String,
    fields: Vec<SignatureField>,
}

// Find all interface imports in the world WIT file
fn find_interfaces_in_world(api_dir: &Path) -> Result<Vec<String>> {
    let mut interfaces = Vec::new();
    
    // Find world definition files
    for entry in WalkDir::new(api_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        
        if path.is_file() && path.extension().map_or(false, |ext| ext == "wit") {
            if let Ok(content) = fs::read_to_string(path) {
                if content.contains("world ") {
                    println!("Analyzing world definition file: {}", path.display());
                    
                    // Extract import statements
                    for line in content.lines() {
                        let line = line.trim();
                        if line.starts_with("import ") && line.ends_with(";") {
                            let interface = line
                                .trim_start_matches("import ")
                                .trim_end_matches(";")
                                .trim();
                            
                            interfaces.push(interface.to_string());
                            println!("  Found interface import: {}", interface);
                        }
                    }
                }
            }
        }
    }
    
    Ok(interfaces)
}

// Parse WIT file to extract function signatures and type definitions
fn parse_wit_file(file_path: &Path) -> Result<(Vec<SignatureStruct>, Vec<String>)> {
    println!("Parsing WIT file: {}", file_path.display());
    
    let content = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read WIT file: {}", file_path.display()))?;
    
    let mut signatures = Vec::new();
    let mut type_names = Vec::new();
    
    // Simple parser for WIT files to extract record definitions and types
    let lines: Vec<_> = content.lines().collect();
    let mut i = 0;
    
    while i < lines.len() {
        let line = lines[i].trim();
        
        // Look for record definitions that aren't signature structs
        if line.starts_with("record ") && !line.contains("-signature-") {
            let record_name = line.trim_start_matches("record ").trim_end_matches(" {").trim();
            println!("  Found type: record {}", record_name);
            type_names.push(record_name.to_string());
        }
        // Look for variant definitions (enums)
        else if line.starts_with("variant ") {
            let variant_name = line.trim_start_matches("variant ").trim_end_matches(" {").trim();
            println!("  Found type: variant {}", variant_name);
            type_names.push(variant_name.to_string());
        }
        // Look for signature record definitions
        else if line.starts_with("record ") && line.contains("-signature-") {
            let record_name = line.trim_start_matches("record ").trim_end_matches(" {").trim();
            println!("  Found record: {}", record_name);
            
            // Extract function name and attribute type
            let parts: Vec<_> = record_name.split("-signature-").collect();
            if parts.len() != 2 {
                println!("    Unexpected record name format");
                i += 1;
                continue;
            }
            
            let function_name = parts[0].to_string();
            let attr_type = parts[1].to_string();
            
            // Parse fields
            let mut fields = Vec::new();
            i += 1;
            
            while i < lines.len() && !lines[i].trim().starts_with("}") {
                let field_line = lines[i].trim();
                
                // Skip comments and empty lines
                if field_line.starts_with("//") || field_line.is_empty() {
                    i += 1;
                    continue;
                }
                
                // Parse field definition
                let field_parts: Vec<_> = field_line.split(':').collect();
                if field_parts.len() == 2 {
                    let field_name = field_parts[0].trim().to_string();
                    let field_type = field_parts[1].trim().trim_end_matches(',').to_string();
                    
                    println!("    Field: {} -> {}", field_name, field_type);
                    fields.push(SignatureField {
                        name: field_name,
                        wit_type: field_type,
                    });
                }
                
                i += 1;
            }
            
            signatures.push(SignatureStruct {
                function_name,
                attr_type,
                fields,
            });
        }
        
        i += 1;
    }
    
    println!("Extracted {} signature structs and {} type definitions from {}", 
             signatures.len(), type_names.len(), file_path.display());
    Ok((signatures, type_names))
}

// Generate a Rust async function from a signature struct
fn generate_async_function(signature: &SignatureStruct) -> String {
    // Convert function name from kebab-case to snake_case
    let snake_function_name = to_snake_case(&signature.function_name);
    
    // Get pascal case version for the JSON request format
    let pascal_function_name = to_pascal_case(&signature.function_name);
    
    // Function full name with attribute type
    let full_function_name = format!("{}_{}_rpc", snake_function_name, signature.attr_type);
    
    // Extract parameters and return type
    let mut params = Vec::new();
    let mut param_names = Vec::new();
    let mut return_type = "()".to_string();
    let mut target_param = "";
    
    for field in &signature.fields {
        let field_name_snake = to_snake_case(&field.name);
        let rust_type = wit_type_to_rust(&field.wit_type);
        
        if field.name == "target" {
            if field.wit_type == "string" {
                target_param = "&str";
            } else {
                // Use hyperware_process_lib::Address instead of WitAddress
                target_param = "&Address";
            }
        } else if field.name == "returning" {
            return_type = rust_type;
        } else {
            params.push(format!("{}: {}", field_name_snake, rust_type));
            param_names.push(field_name_snake);
        }
    }
    
    // First parameter is always target
    let all_params = if target_param.is_empty() {
        params.join(", ")
    } else {
        format!("target: {}{}", target_param, if params.is_empty() { "" } else { ", " }) + &params.join(", ")
    };
    
    // Wrap the return type in SendResult
    let wrapped_return_type = format!("SendResult<{}>", return_type);
    
    // For HTTP endpoints, generate commented-out implementation
    if signature.attr_type == "http" {
        let default_value = generate_default_value(&return_type);
        
        // Add underscore prefix to all parameters for HTTP stubs
        let all_params_with_underscore = if target_param.is_empty() {
            params.iter()
                .map(|param| {
                    let parts: Vec<&str> = param.split(':').collect();
                    if parts.len() == 2 {
                        format!("_{}: {}", parts[0], parts[1])
                    } else {
                        format!("_{}", param)
                    }
                })
                .collect::<Vec<String>>()
                .join(", ")
        } else {
            let target_with_underscore = format!("_target: {}", target_param);
            if params.is_empty() {
                target_with_underscore
            } else {
                let params_with_underscore = params.iter()
                    .map(|param| {
                        let parts: Vec<&str> = param.split(':').collect();
                        if parts.len() == 2 {
                            format!("_{}: {}", parts[0], parts[1])
                        } else {
                            format!("_{}", param)
                        }
                    })
                    .collect::<Vec<String>>()
                    .join(", ");
                format!("{}, {}", target_with_underscore, params_with_underscore)
            }
        };
        
        return format!(
            "/// Generated stub for `{}` {} RPC call\n/// HTTP endpoint - uncomment to implement\n// pub async fn {}({}) -> {} {{\n//     // TODO: Implement HTTP endpoint\n//     SendResult::Success({})\n// }}",
            signature.function_name,
            signature.attr_type,
            full_function_name,
            all_params_with_underscore,
            wrapped_return_type,
            default_value
        );
    }
    
    // Format JSON parameters correctly
    let json_params = if param_names.is_empty() {
        // No parameters case
        format!("json!({{\"{}\" : {{}}}})", pascal_function_name)
    } else if param_names.len() == 1 {
        // Single parameter case
        format!("json!({{\"{}\": {}}})", pascal_function_name, param_names[0])
    } else {
        // Multiple parameters case - use tuple format
        format!("json!({{\"{}\": ({})}})", 
                pascal_function_name, 
                param_names.join(", "))
    };
    
    // Generate function with implementation using send
    format!(
        "/// Generated stub for `{}` {} RPC call\npub async fn {}({}) -> {} {{\n    let request = {};\n    send::<{}>(&request, target, 30).await\n}}",
        signature.function_name,
        signature.attr_type,
        full_function_name,
        all_params,
        wrapped_return_type,
        json_params,
        return_type
    )
}

// Create the caller-utils crate with a single lib.rs file
fn create_caller_utils_crate(api_dir: &Path, base_dir: &Path) -> Result<()> {
    // Path to the new crate
    let caller_utils_dir = base_dir.join("caller-utils");
    println!("Creating caller-utils crate at {}", caller_utils_dir.display());
    
    // Create directories
    fs::create_dir_all(&caller_utils_dir)?;
    fs::create_dir_all(caller_utils_dir.join("src"))?;
    println!("Created project directory structure");
    
    // Create Cargo.toml with updated dependencies
    let cargo_toml = r#"[package]
name = "caller-utils"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
anyhow = "1.0"
hyperware_process_lib = { version = "1.0.4", features = ["logging"] }
process_macros = "0.1.0"
futures-util = "0.3"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
hyperware_app_common = { git = "https://github.com/hyperware-ai/hyperprocess-macro" }
once_cell = "1.20.2"
futures = "0.3"
uuid = { version = "1.0" }
wit-bindgen = "0.41.0"

[lib]
crate-type = ["cdylib", "lib"]
"#;
    
    fs::write(caller_utils_dir.join("Cargo.toml"), cargo_toml)
        .with_context(|| "Failed to write caller-utils Cargo.toml")?;
    
    println!("Created Cargo.toml for caller-utils");
    
    // Get the world name (preferably the types- version)
    let world_name = find_world_name(api_dir)?;
    println!("Using world name for code generation: {}", world_name);
    
    // Get all interfaces from the world file
    let interface_imports = find_interfaces_in_world(api_dir)?;
    
    // Store all types from each interface
    let mut interface_types: HashMap<String, Vec<String>> = HashMap::new();
    
    // Find all WIT files in the api directory to generate stubs
    let mut wit_files = Vec::new();
    for entry in WalkDir::new(api_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.is_file() && path.extension().map_or(false, |ext| ext == "wit") {
            // Exclude world definition files
            if let Ok(content) = fs::read_to_string(path) {
                if !content.contains("world ") {
                    wit_files.push(path.to_path_buf());
                }
            }
        }
    }
    
    println!("Found {} WIT interface files", wit_files.len());
    
    // Generate content for each module and collect types
    let mut module_contents = HashMap::<String, String>::new();
    
    for wit_file in &wit_files {
        // Extract the interface name from the file name
        let interface_name = wit_file.file_stem().unwrap().to_string_lossy();
        let snake_interface_name = to_snake_case(&interface_name);
        
        println!("Processing interface: {} -> {}", interface_name, snake_interface_name);
        
        // Parse the WIT file to extract signature structs and types
        match parse_wit_file(wit_file) {
            Ok((signatures, types)) => {
                // Store types for this interface
                interface_types.insert(interface_name.to_string(), types);
                
                if signatures.is_empty() {
                    println!("No signatures found in {}", wit_file.display());
                    continue;
                }
                
                // Generate module content
                let mut mod_content = String::new();
                
                // Add function implementations
                for signature in &signatures {
                    let function_impl = generate_async_function(signature);
                    mod_content.push_str(&function_impl);
                    mod_content.push_str("\n\n");
                }
                
                // Store the module content
                module_contents.insert(snake_interface_name, mod_content);
                
                println!("Generated module content with {} function stubs", signatures.len());
            },
            Err(e) => {
                println!("Error parsing WIT file {}: {}", wit_file.display(), e);
            }
        }
    }
    
    // Create import statements for each interface using "hyperware::process::{interface_name}::*"
    // Use a HashSet to track which interfaces we've already processed to avoid duplicates
    let mut processed_interfaces = std::collections::HashSet::new();
    let mut interface_use_statements = Vec::new();
    
    for interface_name in &interface_imports {
        // Convert to snake case for module name
        let snake_interface_name = to_snake_case(interface_name);
        
        // Only add the import if we haven't processed this interface yet
        if processed_interfaces.insert(snake_interface_name.clone()) {
            // Create wildcard import for this interface
            interface_use_statements.push(
                format!("pub use crate::hyperware::process::{}::*;", snake_interface_name)
            );
        }
    }
    
    // Create single lib.rs with all modules inline
    let mut lib_rs = String::new();
    
    // Updated wit_bindgen usage with explicit world name - FIXED: Removed unused imports
    lib_rs.push_str("wit_bindgen::generate!({\n");
    lib_rs.push_str("    path: \"target/wit\",\n");
    lib_rs.push_str(&format!("    world: \"{}\",\n", world_name));
    lib_rs.push_str("    generate_unused_types: true,\n");
    lib_rs.push_str("    additional_derives: [serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],\n");
    lib_rs.push_str("});\n\n");
    
    lib_rs.push_str("/// Generated caller utilities for RPC function stubs\n\n");
    
    // Add global imports
    lib_rs.push_str("pub use hyperware_app_common::SendResult;\n");
    lib_rs.push_str("pub use hyperware_app_common::send;\n");
    lib_rs.push_str("use hyperware_process_lib::Address;\n");
    lib_rs.push_str("use serde_json::json;\n\n");
    
    // Add interface use statements
    if !interface_use_statements.is_empty() {
        lib_rs.push_str("// Import types from each interface\n");
        for use_stmt in interface_use_statements {
            lib_rs.push_str(&format!("{}\n", use_stmt));
        }
        lib_rs.push_str("\n");
    }
    
    // Add all modules with their content
    for (module_name, module_content) in module_contents {
        lib_rs.push_str(&format!("/// Generated RPC stubs for the {} interface\n", module_name));
        lib_rs.push_str(&format!("pub mod {} {{\n", module_name));
        lib_rs.push_str("    use crate::*;\n\n");
        lib_rs.push_str(&format!("    {}\n", module_content.replace("\n", "\n    ")));
        lib_rs.push_str("}\n\n");
    }
    
    // Write lib.rs
    let lib_rs_path = caller_utils_dir.join("src").join("lib.rs");
    println!("Writing lib.rs to {}", lib_rs_path.display());
    
    fs::write(&lib_rs_path, lib_rs)
        .with_context(|| format!("Failed to write lib.rs: {}", lib_rs_path.display()))?;
    
    println!("Created single lib.rs file with all modules inline");
    
    // Create target/wit directory and copy all WIT files
    let target_wit_dir = caller_utils_dir.join("target").join("wit");
    println!("Creating directory: {}", target_wit_dir.display());
    
    // Remove the directory if it exists to ensure clean state
    if target_wit_dir.exists() {
        println!("Removing existing target/wit directory");
        fs::remove_dir_all(&target_wit_dir)?;
    }
    
    fs::create_dir_all(&target_wit_dir)?;
    
    // Copy all WIT files to target/wit
    for entry in WalkDir::new(api_dir)
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.is_file() && path.extension().map_or(false, |ext| ext == "wit") {
            let file_name = path.file_name().unwrap();
            let target_path = target_wit_dir.join(file_name);
            fs::copy(path, &target_path)
                .with_context(|| format!("Failed to copy {} to {}", path.display(), target_path.display()))?;
            println!("Copied {} to target/wit directory", file_name.to_string_lossy());
        }
    }
    
    Ok(())
}

// Update workspace Cargo.toml to include the caller-utils crate
fn update_workspace_cargo_toml(base_dir: &Path) -> Result<()> {
    let workspace_cargo_toml = base_dir.join("Cargo.toml");
    println!("Updating workspace Cargo.toml at {}", workspace_cargo_toml.display());
    
    if !workspace_cargo_toml.exists() {
        println!("Workspace Cargo.toml not found at {}", workspace_cargo_toml.display());
        return Ok(());
    }
    
    let content = fs::read_to_string(&workspace_cargo_toml)
        .with_context(|| format!("Failed to read workspace Cargo.toml: {}", workspace_cargo_toml.display()))?;
    
    // Parse the TOML content
    let mut parsed_toml: Value = content.parse()
        .with_context(|| "Failed to parse workspace Cargo.toml")?;
    
    // Check if there's a workspace section
    if let Some(workspace) = parsed_toml.get_mut("workspace") {
        if let Some(members) = workspace.get_mut("members") {
            if let Some(members_array) = members.as_array_mut() {
                // Check if caller-utils is already in the members list
                let caller_utils_exists = members_array.iter().any(|m| {
                    m.as_str().map_or(false, |s| s == "caller-utils")
                });
                
                if !caller_utils_exists {
                    println!("Adding caller-utils to workspace members");
                    members_array.push(Value::String("caller-utils".to_string()));
                    
                    // Write back the updated TOML
                    let updated_content = toml::to_string_pretty(&parsed_toml)
                        .with_context(|| "Failed to serialize updated workspace Cargo.toml")?;
                    
                    fs::write(&workspace_cargo_toml, updated_content)
                        .with_context(|| format!("Failed to write updated workspace Cargo.toml: {}", workspace_cargo_toml.display()))?;
                    
                    println!("Successfully updated workspace Cargo.toml");
                } else {
                    println!("caller-utils is already in workspace members");
                }
            }
        }
    }
    
    Ok(())
}

// Add caller-utils as a dependency to hyperware:process crates
fn add_caller_utils_to_projects(projects: &[PathBuf]) -> Result<()> {
    for project_path in projects {
        let cargo_toml_path = project_path.join("Cargo.toml");
        println!("Adding caller-utils dependency to {}", cargo_toml_path.display());
        
        let content = fs::read_to_string(&cargo_toml_path)
            .with_context(|| format!("Failed to read project Cargo.toml: {}", cargo_toml_path.display()))?;
        
        let mut parsed_toml: Value = content.parse()
            .with_context(|| format!("Failed to parse project Cargo.toml: {}", cargo_toml_path.display()))?;
        
        // Add caller-utils to dependencies if not already present
        if let Some(dependencies) = parsed_toml.get_mut("dependencies") {
            if let Some(deps_table) = dependencies.as_table_mut() {
                if !deps_table.contains_key("caller-utils") {
                    deps_table.insert(
                        "caller-utils".to_string(),
                        Value::Table({
                            let mut t = toml::map::Map::new();
                            t.insert("path".to_string(), Value::String("../caller-utils".to_string()));
                            t
                        })
                    );
                    
                    // Write back the updated TOML
                    let updated_content = toml::to_string_pretty(&parsed_toml)
                        .with_context(|| format!("Failed to serialize updated project Cargo.toml: {}", cargo_toml_path.display()))?;
                    
                    fs::write(&cargo_toml_path, updated_content)
                        .with_context(|| format!("Failed to write updated project Cargo.toml: {}", cargo_toml_path.display()))?;
                    
                    println!("Successfully added caller-utils dependency");
                } else {
                    println!("caller-utils dependency already exists");
                }
            }
        }
    }
    
    Ok(())
}

// Create caller-utils crate and integrate with the workspace
pub fn create_caller_utils(base_dir: &Path, api_dir: &Path, projects: &[PathBuf]) -> Result<()> {
    // Step 1: Create the caller-utils crate
    create_caller_utils_crate(api_dir, base_dir)?;
    
    // Step 2: Update workspace Cargo.toml
    update_workspace_cargo_toml(base_dir)?;
    
    // Step 3: Add caller-utils dependency to each hyperware:process project
    add_caller_utils_to_projects(projects)?;
    
    Ok(())
}