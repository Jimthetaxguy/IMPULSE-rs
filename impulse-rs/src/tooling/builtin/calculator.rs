//! Calculator tool — wraps tools::python::calculate()

use async_trait::async_trait;

use crate::tooling::error::ToolError;
use crate::tooling::traits::*;

pub struct CalculatorTool;

#[async_trait]
impl DynamicTool for CalculatorTool {
    fn id(&self) -> &str {
        "calculator"
    }

    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: "calculator".into(),
            name: "Calculator".into(),
            description: "Evaluate a mathematical expression (digits and + - * / % ( ) . e/E, whitespace; no names, sqrt, or abs). Times out after 5s.".into(),
            version: "0.1.0".into(),
            category: ToolCategory::Utility,
            params: vec![ToolParam {
                name: "expression".into(),
                description: "Mathematical formula only: digits, + - * / % ( ) . e/E, whitespace. Example: '2 + 2', '(10 * 5) / 3'. No identifiers."
                    .into(),
                param_type: ParamType::String,
                required: true,
                default: None,
            }],
        }
    }

    fn validate_params(&self, params: &serde_json::Value) -> Result<(), ToolError> {
        match params.get("expression").and_then(|v| v.as_str()) {
            Some(expr) if crate::tools::python::is_restricted_math_expression(expr) => Ok(()),
            Some(_) => Err(ToolError::InvalidParams(
                "expression must be a mathematical formula (digits and + - * / % ( ) . e), not Python"
                    .into(),
            )),
            None => Err(ToolError::InvalidParams(
                "missing or empty 'expression' string".into(),
            )),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let expression = params
            .get("expression")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParams("missing 'expression'".into()))?;

        match crate::tools::python::calculate(expression) {
            Ok(result) => Ok(ToolResult::json(serde_json::json!({
                "expression": expression,
                "result": result,
            }))),
            Err(e) => Err(ToolError::ExecutionFailed(format!(
                "Calculation failed: {}",
                e
            ))),
        }
    }

    fn required_capabilities(&self) -> Vec<Capability> {
        vec![Capability::PythonExec]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor() {
        let tool = CalculatorTool;
        let desc = tool.descriptor();
        assert_eq!(desc.id, "calculator");
        assert_eq!(desc.category, ToolCategory::Utility);
        assert_eq!(desc.params.len(), 1);
        assert!(
            desc.description.contains("digits") && desc.description.contains("no names"),
            "descriptor must name the math grammar, got {}",
            desc.description
        );
    }

    #[test]
    fn test_validate_params_ok() {
        let tool = CalculatorTool;
        let params = serde_json::json!({"expression": "2 + 2"});
        assert!(tool.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_params_missing() {
        let tool = CalculatorTool;
        let params = serde_json::json!({});
        assert!(tool.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_empty() {
        let tool = CalculatorTool;
        let params = serde_json::json!({"expression": ""});
        assert!(tool.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_rejects_python_injection() {
        let tool = CalculatorTool;
        let params = serde_json::json!({"expression": "__import__('os').system('id')"});
        let err = tool
            .validate_params(&params)
            .expect_err("calculator must not accept arbitrary Python");
        let msg = err.to_string();
        assert!(msg.contains("mathematical formula"), "got {msg}");
    }

    #[tokio::test]
    async fn test_execute() {
        let tool = CalculatorTool;
        let ctx = ToolContext::with_all_capabilities();
        let params = serde_json::json!({"expression": "2 + 2"});
        let result = tool.execute(params, &ctx).await;
        // May fail if Python not available, but should not panic
        if let Ok(r) = result {
            assert!(r.output.get("result").is_some());
        }
    }
}
