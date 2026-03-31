# Adding Dynamic Tools to Impulse

Step-by-step guide for adding a new tool to the DynamicTool registry.

---

## Steps

### 1. Create the tool module

In `src/tooling/` or a domain-specific module:

```rust
use async_trait::async_trait;
use serde_json::Value;

use crate::tooling::{
    DynamicTool, ToolCapability, ToolContext, ToolDescriptor, ToolParam, ToolResult,
};

pub struct MyNewTool;

#[async_trait]
impl DynamicTool for MyNewTool {
    fn name(&self) -> &str {
        "my-new-tool"
    }

    fn description(&self) -> &str {
        "Brief description of what the tool does"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: self.name().to_string(),
            description: self.description().to_string(),
            params: vec![
                ToolParam {
                    name: "input".to_string(),
                    param_type: "string".to_string(),
                    description: "The input to process".to_string(),
                    required: true,
                    default: None,
                },
                ToolParam {
                    name: "format".to_string(),
                    param_type: "string".to_string(),
                    description: "Output format (text or json)".to_string(),
                    required: false,
                    default: Some(Value::String("text".to_string())),
                },
            ],
            required_capability: ToolCapability::ReadOnly,
            category: Some("analysis".to_string()),
        }
    }

    fn required_capability(&self) -> ToolCapability {
        ToolCapability::ReadOnly
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> ToolResult {
        // Extract and validate params
        let input = params.get("input")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing required param: input".to_string())?;

        let format = params.get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("text");

        // Do the work (within ctx constraints)
        let result = process_input(input)?;

        // Return result
        Ok(serde_json::json!({
            "output": result,
            "format": format,
        }))
    }
}
```

### 2. Register the tool

In `src/tooling/registry.rs` where tools are registered:

```rust
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    // ... existing tools ...
    registry.register(Box::new(MyNewTool));
}
```

### 3. Wire the capability

The registry enforces: **exists → capability check → param validation → execute**

Capability levels (deny-by-default):
| Capability | Allows |
|---|---|
| `ReadOnly` | Read files, search, query |
| `WriteLocal` | Write to `.impulse/` directory |
| `WriteProject` | Write anywhere in project |
| `Execute` | Run external commands |
| `Network` | Make network requests |

Set `required_capability()` to the minimum level needed. Users must grant the
capability in their config before the tool will execute.

### 4. Add tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_my_new_tool_basic() {
        let tool = MyNewTool;
        let ctx = ToolContext::default();
        let params = serde_json::json!({
            "input": "test data"
        });

        let result = tool.execute(params, &ctx).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.get("output").is_some());
    }

    #[tokio::test]
    async fn test_my_new_tool_missing_required_param() {
        let tool = MyNewTool;
        let ctx = ToolContext::default();
        let params = serde_json::json!({});

        let result = tool.execute(params, &ctx).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_my_new_tool_descriptor() {
        let tool = MyNewTool;
        let desc = tool.descriptor();
        assert_eq!(desc.name, "my-new-tool");
        assert!(!desc.params.is_empty());
        assert_eq!(desc.required_capability, ToolCapability::ReadOnly);
    }
}
```

### 5. Expose via three surfaces

Dynamic tools are automatically available on all three invocation surfaces:

1. **CLI**: `impulse-rs tooling-run --tool my-new-tool --params '{"input":"test"}'`
2. **Daemon IPC**: `InvokeTool { name: "my-new-tool", params: {...} }`
3. **Schema export**: `impulse-rs tool-schema` includes the tool's descriptor

No additional wiring needed — the registry handles dispatch.

---

## Checklist

- [ ] Tool struct implementing `DynamicTool` trait
- [ ] `name()`, `description()`, `descriptor()` implemented
- [ ] `required_capability()` set to minimum needed
- [ ] `execute()` validates params and returns `ToolResult`
- [ ] Tool registered in `register_builtin_tools()`
- [ ] Test for basic execution
- [ ] Test for missing required param
- [ ] Test for descriptor correctness
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` clean
