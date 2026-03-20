import { defineConfig } from "astro/config";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  site: "https://decodex.space",
  vite: {
    plugins: [tailwindcss()],
  },
});
