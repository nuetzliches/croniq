import { defineConfig } from 'vitepress';

export default defineConfig({
    lang: 'en-US',
    title: 'Croniq Docs',
    description: 'Croniq consumer + deep-dive documentation',
    srcDir: '.',
    outDir: './.vitepress/dist',
    cleanUrls: true,
    lastUpdated: true,
    head: [
        ['meta', { name: 'theme-color', content: '#0f172a' }]
    ],
    markdown: {
        theme: {
            light: 'github-light',
            dark: 'github-dark'
        }
    },
    mermaid: true,
    themeConfig: {
        siteTitle: 'Croniq Docs',
        nav: [
            { text: 'Introduction', link: '/introduction/' },
            { text: 'Quickstart', link: '/introduction/quickstart' },
            { text: 'Configuration', link: '/introduction/configuration' },
            { text: 'Deep Dive', link: '/deep-dive/' }
        ],
        sidebar: {
            '/': [
                {
                    text: 'Introduction',
                    items: [
                        { text: 'What is Croniq?', link: '/introduction/' },
                        { text: 'Quickstart', link: '/introduction/quickstart' },
                        { text: 'Configuration', link: '/introduction/configuration' }
                    ]
                },
                {
                    text: 'Guides',
                    items: [
                        { text: 'Authentication', link: '/guides/auth' },
                        { text: 'Policies', link: '/guides/policies' },
                        { text: 'Triggers', link: '/guides/triggers' },
                        { text: 'Handlers', link: '/guides/handlers' }
                    ]
                },
                {
                    text: 'Operations',
                    items: [
                        { text: 'Troubleshooting', link: '/ops/troubleshooting' }
                    ]
                }
            ],
            '/deep-dive/': [
                {
                    text: 'Deep Dive',
                    items: [
                        { text: 'Overview', link: '/deep-dive/' },
                        { text: 'Doc Streams Plan', link: '/deep-dive/docstreams' },
                        { text: 'CI / Release', link: '/deep-dive/ci' },
                        { text: 'Dev Stack', link: '/deep-dive/devstack' },
                        { text: 'Testing', link: '/deep-dive/testing' },
                        { text: 'Observability', link: '/deep-dive/observability' },
                        { text: 'Policies', link: '/deep-dive/policies' },
                        { text: 'Security', link: '/deep-dive/security' },
                        { text: 'Supply Chain', link: '/deep-dive/supplychain' },
                        { text: 'Kubernetes Plan', link: '/deep-dive/kubernetes' },
                        { text: 'UI Backlog', link: '/deep-dive/ui' }
                    ]
                }
            ]
        },
        socialLinks: [
            { icon: 'github', link: 'https://github.com/nuetzliches/croniq' }
        ]
    }
});
