//! ToolRegistry — central registration, discovery, and dispatch for dynamic tools
//!
//! Inspired by DataFusion's SessionContext pattern: register once, query/execute anywhere.

use std::collections::HashMap;

use super::error::ToolError;
use super::traits::{DynamicTool, ToolCategory, ToolContext, ToolDescriptor, ToolResult};

/// Central registry for all dynamic tools.
///
/// Tools are registered by ID and dispatched through `execute()`.
/// The registry enforces capability checks before execution.
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn DynamicTool>>,
}

impl ToolRegistry {
    /// Create an empty registry
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. Returns error if ID is already taken.
    pub fn register(&mut self, tool: Box<dyn DynamicTool>) -> Result<(), ToolError> {
        let id = tool.id().to_string();
        if self.tools.contains_key(&id) {
            return Err(ToolError::AlreadyRegistered(id));
        }
        self.tools.insert(id, tool);
        Ok(())
    }

    /// Get a tool by ID
    pub fn get(&self, id: &str) -> Option<&dyn DynamicTool> {
        self.tools.get(id).map(|t| t.as_ref())
    }

    /// List all registered tool descriptors
    pub fn list(&self) -> Vec<ToolDescriptor> {
        let mut descriptors: Vec<_> = self.tools.values().map(|t| t.descriptor()).collect();
        descriptors.sort_by(|a, b| a.id.cmp(&b.id));
        descriptors
    }

    /// List tools filtered by category
    pub fn list_by_category(&self, category: ToolCategory) -> Vec<ToolDescriptor> {
        let mut descriptors: Vec<_> = self
            .tools
            .values()
            .map(|t| t.descriptor())
            .filter(|d| d.category == category)
            .collect();
        descriptors.sort_by(|a, b| a.id.cmp(&b.id));
        descriptors
    }

    /// Number of registered tools
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Execute a tool by ID with params and context.
    ///
    /// Enforces: tool exists → capabilities check → param validation → execution.
    pub async fn execute(
        &self,
        id: &str,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let tool = self
            .tools
            .get(id)
            .ok_or_else(|| ToolError::NotFound(id.to_string()))?;

        // Check capabilities
        for cap in tool.required_capabilities() {
            if !ctx.has_capability(cap) {
                return Err(ToolError::MissingCapability(cap));
            }
        }

        // Validate params
        tool.validate_params(&params)?;

        // Execute
        tool.execute(params, ctx).await
    }

    /// Create a registry with all built-in tools registered.
    ///
    /// Panics only if there's a bug in tool ID uniqueness (programming error).
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        super::builtin::register_all(&mut reg)
            .expect("built-in tool registration failed (duplicate IDs)");

        #[cfg(feature = "office-support")]
        super::document::register_all(&mut reg);

        reg
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tooling::traits::*;
    use async_trait::async_trait;

    /// A minimal test tool for registry tests
    struct EchoTool;

    #[async_trait]
    impl DynamicTool for EchoTool {
        fn id(&self) -> &str {
            "echo"
        }
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                id: "echo".into(),
                name: "Echo".into(),
                description: "Echoes input back".into(),
                version: "0.1.0".into(),
                category: ToolCategory::Utility,
                params: vec![ToolParam {
                    name: "text".into(),
                    description: "Text to echo".into(),
                    param_type: ParamType::String,
                    required: true,
                    default: None,
                }],
            }
        }
        fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
            if params.get("text").is_none() {
                return Err(ToolError::InvalidParams("missing 'text'".into()));
            }
            Ok(())
        }
        async fn execute(
            &self,
            params: serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<ToolResult, ToolError> {
            let text = params["text"].as_str().unwrap_or("");
            Ok(ToolResult::text(text))
        }
        fn required_capabilities(&self) -> Vec<Capability> {
            vec![] // Echo needs nothing
        }
    }

    /// A tool that requires filesystem write
    struct WriteTool;

    #[async_trait]
    impl DynamicTool for WriteTool {
        fn id(&self) -> &str {
            "write_test"
        }
        fn descriptor(&self) -> ToolDescriptor {
            ToolDescriptor {
                id: "write_test".into(),
                name: "Write Test".into(),
                description: "Test tool requiring write".into(),
                version: "0.1.0".into(),
                category: ToolCategory::Utility,
                params: vec![],
            }
        }
        fn validate_params(&self, _params: &serde_json::Value) -> Result<(), ToolError> {
            Ok(())
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::text("written"))
        }
        fn required_capabilities(&self) -> Vec<Capability> {
            vec![Capability::FileSystemWrite]
        }
    }

    #[test]
    fn test_register_and_get() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(EchoTool)).unwrap();
        assert_eq!(reg.len(), 1);
        assert!(reg.get("echo").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_duplicate_register_fails() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(EchoTool)).unwrap();
        let result = reg.register(Box::new(EchoTool));
        assert!(result.is_err());
    }

    #[test]
    fn test_list_sorted() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(WriteTool)).unwrap();
        reg.register(Box::new(EchoTool)).unwrap();
        let list = reg.list();
        assert_eq!(list[0].id, "echo");
        assert_eq!(list[1].id, "write_test");
    }

    #[test]
    fn test_list_by_category() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(EchoTool)).unwrap();
        let utils = reg.list_by_category(ToolCategory::Utility);
        assert_eq!(utils.len(), 1);
        let docs = reg.list_by_category(ToolCategory::Document);
        assert_eq!(docs.len(), 0);
    }

    #[tokio::test]
    async fn test_execute_success() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(EchoTool)).unwrap();
        let ctx = ToolContext::default();
        let params = serde_json::json!({"text": "hello"});
        let result = reg.execute("echo", params, &ctx).await.unwrap();
        assert_eq!(result.output, serde_json::Value::String("hello".into()));
    }

    #[tokio::test]
    async fn test_execute_not_found() {
        let reg = ToolRegistry::new();
        let ctx = ToolContext::default();
        let result = reg.execute("missing", serde_json::json!({}), &ctx).await;
        assert!(matches!(result, Err(ToolError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_execute_missing_capability() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(WriteTool)).unwrap();
        // Default context does NOT have FileSystemWrite
        let ctx = ToolContext::default();
        let result = reg.execute("write_test", serde_json::json!({}), &ctx).await;
        assert!(matches!(result, Err(ToolError::MissingCapability(_))));
    }

    #[tokio::test]
    async fn test_execute_with_all_capabilities() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(WriteTool)).unwrap();
        let ctx = ToolContext::with_all_capabilities();
        let result = reg.execute("write_test", serde_json::json!({}), &ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_invalid_params() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(EchoTool)).unwrap();
        let ctx = ToolContext::default();
        // Missing required "text" param
        let result = reg.execute("echo", serde_json::json!({}), &ctx).await;
        assert!(matches!(result, Err(ToolError::InvalidParams(_))));
    }

    #[test]
    fn test_with_defaults() {
        let reg = ToolRegistry::with_defaults();
        assert!(!reg.is_empty());
    }
}
