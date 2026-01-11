/** @type { import('tailwindcss').Config } */
module.exports = {
    content: ['./src/**/*.{html,ts}', './projects/**/*.{html,ts}'],
    theme: {
        extend: {
            colors: {
                surface: {
                    DEFAULT: 'var(--cq-surface)',
                    alt: 'var(--cq-surface-alt)',
                    1: 'var(--cq-surface-1)',
                    2: 'var(--cq-surface-2)',
                    3: 'var(--cq-surface-3)',
                    4: 'var(--cq-surface-4)',
                    5: 'var(--cq-surface-5)',
                },
                primary: 'var(--cq-text-primary)',
                muted: 'var(--cq-text-secondary)',
                border: 'var(--cq-border)',
                accent: {
                    DEFAULT: 'var(--cq-accent)',
                    hover: 'var(--cq-accent-strong)',
                    1: 'var(--cq-accent-1)',
                    2: 'var(--cq-accent-2)',
                    3: 'var(--cq-accent-3)',
                    4: 'var(--cq-accent-4)',
                    5: 'var(--cq-accent-5)',
                },
                danger: {
                    DEFAULT: 'var(--cq-danger)',
                    1: 'var(--cq-danger-1)',
                    2: 'var(--cq-danger-2)',
                    3: 'var(--cq-danger-3)',
                    4: 'var(--cq-danger-4)',
                    5: 'var(--cq-danger-5)',
                },
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
            },
            transitionDuration: {
                fast: 'var(--cq-motion-fast)',
                medium: 'var(--cq-motion-medium)',
                emphasis: 'var(--cq-motion-emphasis)',
            },
            transitionTimingFunction: {
                standard: 'var(--cq-motion-ease)',
                emphasis: 'var(--cq-motion-ease-emphasis)',
            },
            keyframes: {
                'panel-sweep': {
                    '0%': { opacity: '0', transform: 'translateY(12px) scale(0.98)' },
                    '100%': { opacity: '1', transform: 'translateY(0) scale(1)' },
                },
                'counter-flip': {
                    '0%': { opacity: '0', transform: 'translateY(8px) rotateX(-75deg)' },
                    '100%': { opacity: '1', transform: 'translateY(0) rotateX(0deg)' },
                },
                reveal: {
                    '0%': { opacity: '0', transform: 'translateY(6px)' },
                    '100%': { opacity: '1', transform: 'translateY(0)' },
                },
            },
            animation: {
                'panel-sweep': 'panel-sweep var(--cq-motion-medium) var(--cq-motion-ease) both',
                'counter-flip': 'counter-flip var(--cq-motion-emphasis) var(--cq-motion-ease-emphasis) both',
                reveal: 'reveal var(--cq-motion-medium) var(--cq-motion-ease) both',
                'reveal-fast': 'reveal var(--cq-motion-fast) var(--cq-motion-ease) both',
            },
            fontFamily: {
                sans: ['"Space Grotesk"', 'sans-serif'],
                mono: ['"IBM Plex Mono"', 'monospace'],
            },
        },
    },
    plugins: [],
};
