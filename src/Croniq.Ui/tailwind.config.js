/** @type { import('tailwindcss').Config } */
module.exports = {
    content: ['./src/**/*.{html,ts}', './projects/**/*.{html,ts}'],
    theme: {
        extend: {
            colors: {
                surface: {
                    DEFAULT: 'var(--cq-surface, #070d1a)',
                    alt: 'var(--cq-surface-alt, #111a2d)',
                },
                text: {
                    DEFAULT: 'var(--cq-text, #f8fafc)',
                    muted: 'var(--cq-text-muted, #94a3b8)',
                },
                accent: {
                    DEFAULT: 'var(--cq-accent, #27d2ff)',
                },
            },
        },
    },
    plugins: [],
};
