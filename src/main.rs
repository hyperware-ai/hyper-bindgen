use anyhow::Result;

mod wit_generator;
mod caller_utils_generator;

fn main() -> Result<()> {
    // Get the current working directory
    let cwd = std::env::current_dir()?;
    println!("Current working directory: {}", cwd.display());
    
    // Create the api directory if it doesn't exist
    let api_dir = cwd.join("api");
    println!("API directory: {}", api_dir.display());
    
    std::fs::create_dir_all(&api_dir)?;
    println!("Created or verified api directory");
    
    // Step 1: Generate WIT files from Rust code
    println!("\n=== STEP 1: Generating WIT Files ===");
    let (processed_projects, interfaces) = wit_generator::generate_wit_files(&cwd, &api_dir)?;
    
    if processed_projects.is_empty() {
        println!("No relevant Rust projects found with hyperware:process metadata.");
        return Ok(());
    }
    
    // Step 2: Create caller-utils crate with stubs
    println!("\n=== STEP 2: Generating Caller Utils Crate ===");
    if !interfaces.is_empty() {
        caller_utils_generator::create_caller_utils(&cwd, &api_dir, &processed_projects)?;
    } else {
        println!("No interfaces found, skipping caller-utils creation");
    }
    
    // Print summary
    println!("\n=== Summary ===");
    println!("- Processed {} Rust projects", processed_projects.len());
    println!("- Generated {} WIT interface files", interfaces.len());
    if !interfaces.is_empty() {
        println!("- Created caller-utils crate with stub implementations");
        println!("- Updated workspace Cargo.toml");
        println!("- Added caller-utils dependency to projects");
    }
    println!("\nAll operations completed successfully!");
    
    Ok(())
}