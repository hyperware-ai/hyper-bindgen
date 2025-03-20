# Hyper-Bindgen

Automates the creation of WIT (WebAssembly Interface Type) files and RPC stubs for Hyperware processes.

## Installation

```bash
# Install from the current directory
cargo install --path .
```

## Usage

Run this tool before building your Hyperware project:

```bash
# Navigate to your project root
cd my-hyperware-project

# Run hyper-bindgen
hyper-bindgen
# The tool will:
# 1. Find all Rust files with hyperprocess implementations
# 2. Generate corresponding WIT files in the api/ directory
# 3. Create the caller-utils crate with RPC stubs

# Then build as normal
kit b # build
kit s # start (assuming you started a fakenode with `kit f`)
```

## Overview

Hyper-Bindgen scans your codebase for Hyperware processes (identified by the `#[hyperprocess]` macro) and performs two steps:

1. **WIT File Generation**: Creates wit files with function signatures and downstream structs used either in the args or return value for all annotated functions
2. **Caller Utils Generation**: Builds a helper crate with RPC stub functions

This allows you to call other process endpoints from a process through auto-generated async functions with proper type checking, rather than manually constructing JSON messages.

## How It Works

When run in a project, Hyper-Bindgen performs two main tasks:

### WIT File Generation (`wit_generator.rs`):

1. Scans for Rust projects with `package.metadata.component.package = "hyperware:process"` in Cargo.toml
2. Analyzes each project for `#[hyperprocess]` macro implementations
3. Extracts function signatures from methods annotated with `#[http]`, `#[remote]`, or `#[local]`
4. Generates WIT files in an `/api` directory with proper type conversions

### Caller Utils Generation (`caller_utils_generator.rs`):

1. Creates a `caller-utils` crate containing RPC stub functions for easy inter-process communication
2. Updates the workspace Cargo.toml to include the new crate
3. Adds the caller-utils dependency to relevant projects



> **Note:** In the future, we should extend the `kit b` command to automatically execute hyper-bindgen beforehand, eliminating the need for a separate step.


## Example

For a Hyperware process with annotated functions:

```rust
#[hyperprocess(
    name = "Async Requester",
    wit_world = "async-app-template-dot-os-v0"
)]
impl AsyncRequesterState {
    #[remote]
    #[local]
    fn increment_counter(&mut self, value: i32, name: String) -> f32 {
        // Implementation...
        0.0
    }
}
```

Hyper-Bindgen will:

1. Generate a WIT file with:
   ```wit
   interface async-requester {
       use standard.{address};
       
       record increment-counter-signature-remote {
           target: address,
           value: s32,
           name: string,
           returning: f32,
       }
       
       record increment-counter-signature-local {
           target: address, 
           value: s32,
           name: string,
           returning: f32,
       }
   }
   ```

2. Create a caller-utils crate with stub functions:
   ```rust
   pub mod async_requester {
       use crate::*;
       
       /// Generated stub for `increment-counter` remote RPC call
       pub async fn increment_counter_remote_rpc(target: &Address, value: i32, name: String) -> SendResult<f32> {
           let request = json!({"IncrementCounter": (value, name)});
           send::<f32>(&request, target, 30).await
       }
       
       /// Generated stub for `increment-counter` local RPC call  
       pub async fn increment_counter_local_rpc(target: &Address, value: i32, name: String) -> SendResult<f32> {
           let request = json!({"IncrementCounter": (value, name)});
           send::<f32>(&request, target, 30).await
       }
   }
   ```

## Inter-Process Communication

With the generated stubs, you can call another process's endpoint like this:

```rust
use caller_utils::async_requester::increment_counter_remote_rpc;
use shared::receiver_address;

async fn my_function() {
    let result = increment_counter_remote_rpc(&receiver_address(), 42, "test".to_string()).await;
    match result {
        SendResult::Success(value) => println!("Got result: {}", value),
        SendResult::Error(err) => println!("Error: {}", err),
    }
}
```

Instead of manually constructing JSON:

```rust
// Without hyper-bindgen (error-prone)
let request = json!({"IncrementCounter": (42, "test")});
let result = send::<f32>(&request, &receiver_address(), 30).await;
```

Under the hood, we still do the regular sending of data through messages. The body of the messages will always follow the `RequestEnum`/`ResponseEnum` with the variants being a CamelCase version of each defined function, and the inner value of those variants being the arguments of the functions defined in the hyperware macro functions.
