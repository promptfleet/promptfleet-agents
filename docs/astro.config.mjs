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
			],
		}),
	],
});
