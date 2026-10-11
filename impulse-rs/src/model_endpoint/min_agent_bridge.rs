//! Converts a validated [`ModelEndpoint`] into `min-agent`'s connection and
//! profile types. The only place in Impulse that knows those shapes.

use min_agent::config::{Auth, Connection, ModelProfile, Protocol};

use super::{EndpointAuth, EndpointError, ModelEndpoint, WireProtocol};

/// Profile-to-connection link name inside the converted pair; never user-facing.
const CONNECTION_NAME: &str = "impulse-endpoint";

fn protocol(protocol: WireProtocol) -> Protocol {
    match protocol {
        WireProtocol::AnthropicMessages => Protocol::AnthropicMessages,
        WireProtocol::OpenaiChat => Protocol::OpenaiChat,
        WireProtocol::OpenaiResponses => Protocol::OpenaiResponses,
    }
}

fn auth(auth: &EndpointAuth) -> Auth {
    match auth {
        EndpointAuth::None => Auth::None,
        EndpointAuth::BearerEnv { env } => Auth::BearerEnv { env: env.clone() },
        EndpointAuth::HeaderEnv { header, env } => Auth::HeaderEnv {
            header: header.clone(),
            env: env.clone(),
        },
    }
}

/// Validates `endpoint`, then returns the equivalent `min-agent` pair.
pub fn to_min_agent(endpoint: &ModelEndpoint) -> Result<(Connection, ModelProfile), EndpointError> {
    endpoint.validate()?;
    let connection = Connection {
        protocol: protocol(endpoint.protocol),
        base_url: endpoint.base_url.clone(),
        auth: auth(&endpoint.auth),
        proxy: None,
    };
    let profile = ModelProfile {
        connection: CONNECTION_NAME.into(),
        model: endpoint.model.trim().to_string(),
        native_tools: true,
        max_output_tokens: endpoint.max_output_tokens,
        output_limit_parameter: None,
    };
    Ok((connection, profile))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(protocol: WireProtocol, base_url: &str, auth: EndpointAuth) -> ModelEndpoint {
        ModelEndpoint {
            protocol,
            base_url: base_url.into(),
            auth,
            model: " model-x ".into(),
            max_output_tokens: Some(512),
            capabilities: None,
        }
    }

    #[test]
    fn test_to_min_agent_maps_each_protocol_to_its_endpoint_path() {
        let cases = [
            (
                WireProtocol::AnthropicMessages,
                "https://api.anthropic.com/v1",
                "https://api.anthropic.com/v1/messages",
            ),
            (
                WireProtocol::OpenaiChat,
                "https://openrouter.ai/api/v1",
                "https://openrouter.ai/api/v1/chat/completions",
            ),
            (
                WireProtocol::OpenaiResponses,
                "https://api.openai.com/v1",
                "https://api.openai.com/v1/responses",
            ),
        ];
        for (wire, base, expected) in cases {
            let (conn, profile) = to_min_agent(&endpoint(wire, base, EndpointAuth::None)).unwrap();
            assert!(conn.validate().is_ok());
            assert_eq!(conn.endpoint().unwrap().as_str(), expected);
            assert_eq!(profile.model, "model-x");
            assert_eq!(profile.max_output_tokens, Some(512));
        }
    }

    #[test]
    fn test_to_min_agent_carries_auth_by_env_name() {
        let (conn, _) = to_min_agent(&endpoint(
            WireProtocol::OpenaiChat,
            "https://openrouter.ai/api/v1",
            EndpointAuth::BearerEnv {
                env: "OPENROUTER_API_KEY".into(),
            },
        ))
        .unwrap();
        assert!(matches!(&conn.auth, Auth::BearerEnv { env } if env == "OPENROUTER_API_KEY"));
        let (conn, _) = to_min_agent(&endpoint(
            WireProtocol::AnthropicMessages,
            "https://api.anthropic.com/v1",
            EndpointAuth::HeaderEnv {
                header: "x-api-key".into(),
                env: "ANTHROPIC_API_KEY".into(),
            },
        ))
        .unwrap();
        assert!(matches!(&conn.auth, Auth::HeaderEnv { header, .. } if header == "x-api-key"));
    }

    #[test]
    fn test_to_min_agent_refuses_invalid_endpoint() {
        let err = to_min_agent(&endpoint(
            WireProtocol::OpenaiChat,
            "http://openrouter.ai/api/v1",
            EndpointAuth::None,
        ))
        .err()
        .expect("insecure endpoint must be refused");
        assert!(matches!(err, EndpointError::InsecureBaseUrl { .. }));
    }
}
