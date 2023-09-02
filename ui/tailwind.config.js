/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,svelte}",
  ],
  theme: {
    extend: {
      colors: {
        "primary": "#000009",
        "secondary": "#222222",
        "secondary-darker": "#080810",
        "secondary-text": "#333333",
        "secondary-text-lighter": "#404040",
      },
      gridTemplateColumns: {
        "fluid": "repeat(auto-fit, minmax(40rem, 1fr))",
      },
    },
  },
  plugins: [],
};
