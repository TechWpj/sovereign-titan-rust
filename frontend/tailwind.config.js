/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{js,jsx}'],
  theme: {
    extend: {
      colors: {
        'titan-bg': '#0f1117',
        'titan-sidebar': '#14161e',
        'titan-surface': '#1c1f2e',
        'titan-accent': '#6366f1',
        'titan-accent-hover': '#818cf8',
        'titan-border': '#2a2d3e',
        'titan-text': '#e2e8f0',
        'titan-text-muted': '#94a3b8',
      },
    },
  },
  plugins: [require('@tailwindcss/typography')],
};
