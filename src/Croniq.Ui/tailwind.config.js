/** @type { import('tailwindcss').Config } */
module.exports = {
    content: ['./src/**/*.{html,ts}', './projects/**/*.{html,ts}'],
    theme: {
        extend: {
            colors: {
                surface: {
                    DEFAULT: 'var(--cq-surface)',
                    alt: 'var(--cq-surface-alt)',
                },
                primary: 'var(--cq-text-primary)',
                muted: 'var(--cq-text-secondary)',
                border: 'var(--cq-border)',
                accent: {
                    DEFAULT: 'var(--cq-accent)',
                    hover: 'var(--cq-accent-strong)',
                },
                danger: 'var(--cq-danger)',
                warning: 'var(--cq-warning)',
                success: 'var(--cq-success)',
                graph: {
                    1: 'var(--cq-graph-1)',
                    2: 'var(--cq-graph-2)',
                }
            },
            borderRadius: {
                sm: 'var(--cq-radius-sm)',
                lg: 'var(--cq-radius-lg)',
            }
        },
    },
    plugins: [],
};
