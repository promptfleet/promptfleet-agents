// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
	site: 'https://promptfleet.github.io',
	base: '/promptfleet-agents',
	integrations: [
		starlight({
			title: 'PromptFleet Agents',
			description: 'Rust library workspace: SDK, A2A, LLM, observability.',
			editLink: {
				baseUrl: 'https://github.com/promptfleet/promptfleet-agents/edit/main/docs/',
			},
			social: [
				{
					icon: 'github',
					label: 'GitHub',
					href: 'https://github.com/promptfleet/promptfleet-agents',
				},
			],
		sidebar: [
			{ label: 'Getting Started', link: '/getting-started/' },
			{ label: 'Architecture', link: '/architecture/' },
			{
				label: 'SDK / Core',
				items: [
					{ label: 'Overview', link: '/sdk/' },
					{ label: 'agent_sdk', link: '/sdk/agent-sdk/' },
					{ label: 'agent_core', link: '/sdk/agent-core/' },
					{ label: 'pf_macros', link: '/sdk/pf-macros/' },
					{ label: 'pf-types', link: '/sdk/pf-types/' },
					{ label: 'pf_config', link: '/sdk/pf-config/' },
				],
			},
			{
				label: 'A2A Protocol',
				items: [
					{ label: 'Overview', link: '/a2a/' },
					{ label: 'protocol_transport_core', link: '/a2a/protocol-transport-core/' },
					{ label: 'a2a_protocol_core', link: '/a2a/a2a-protocol-core/' },
					{ label: 'a2a_http_client', link: '/a2a/a2a-http-client/' },
					{ label: 'a2a_http_server', link: '/a2a/a2a-http-server/' },
					{ label: 'a2a_app_ports', link: '/a2a/a2a-app-ports/' },
					{ label: 'a2a_rpc_macros', link: '/a2a/a2a-rpc-macros/' },
				],
			},
			{
				label: 'LLM',
				items: [
					{ label: 'Overview', link: '/llm/' },
					{ label: 'llm_client', link: '/llm/llm-client/' },
					{ label: 'llm_context_core', link: '/llm/llm-context-core/' },
					{ label: 'llm_tools', link: '/llm/llm-tools/' },
					{ label: 'llm_tool_macros', link: '/llm/llm-tool-macros/' },
					{ label: 'tool_web_search', link: '/llm/tool-web-search/' },
				],
			},
			{
				label: 'Observability',
				items: [
					{ label: 'Overview', link: '/observability/' },
					{ label: 'observability (facade)', link: '/observability/observability/' },
					{ label: 'observability_core', link: '/observability/observability-core/' },
					{ label: 'structured_logging', link: '/observability/structured-logging/' },
					{ label: 'otel', link: '/observability/otel/' },
					{ label: 'prometheus', link: '/observability/prometheus/' },
				],
			},
			{
				label: 'Support / Integration',
				items: [
					{ label: 'Overview', link: '/support/' },
					{ label: 'foundation_utils', link: '/support/foundation-utils/' },
					{ label: 'mcp_protocol', link: '/support/mcp-protocol/' },
					{ label: 'pf_test_harness', link: '/support/pf-test-harness/' },
					{ label: 'umao_agents', link: '/support/umao-agents/' },
				],
			},
			{
				label: 'API Reference',
				items: [
					{
						label: 'structured_logging',
						items: [
							{ label: 'Overview', link: '/api/structured-logging/' },
							{ label: 'performance', link: '/api/structured-logging/performance/' },
							{ label: 'correlation', link: '/api/structured-logging/correlation/' },
							{ label: 'convenience', link: '/api/structured-logging/convenience/' },
							{ label: 'context_adapter', link: '/api/structured-logging/context-adapter/' },
							{ label: 'panic_handler', link: '/api/structured-logging/panic-handler/' },
							{ label: 'extension', link: '/api/structured-logging/extension/' },
						],
					},
				],
			},
		],
		}),
	],
});
