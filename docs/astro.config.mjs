// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import starlightLinksValidator from "starlight-links-validator";

const ogUrl = new URL("og.png", "https://juanm.dev/ghtui/").href;
const ogImageAlt = "ghtui: GitHub in the terminal";

// https://astro.build/config
export default defineConfig({
  site: "https://juanm.dev",
  base: "/ghtui",
  integrations: [
    starlight({
      title: "ghtui",
      plugins: [starlightLinksValidator()],
      favicon: "/favicon.svg",
      customCss: ["./src/styles/custom.css", "./src/fonts/font-face.css"],
      head: [
        {
          tag: "meta",
          attrs: { property: "og:image", content: ogUrl },
        },
        {
          tag: "meta",
          attrs: { property: "og:image:alt", content: ogImageAlt },
        },
        {
          tag: "meta",
          attrs: {
            name: "description",
            content:
              "ghtui - a rich terminal UI for GitHub that doesn't break your flow",
          },
        },
      ],
      components: {
        ThemeSelect: "./src/components/ThemeSelect.astro",
      },
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/juanibiapina/ghtui",
        },
      ],
      sidebar: [
        {
          label: "Getting Started",
          items: [
            "getting-started",
            "getting-started/usage",
            "getting-started/updating",
          ],
        },
        {
          label: "Keybindings",
          collapsed: true,
          items: [
            { autogenerate: { directory: "getting-started/keybindings" } },
          ],
        },
        {
          label: "Configuration",
          collapsed: true,
          items: [
            "configuration",
            "configuration/schema",
            "configuration/defaults",
            "configuration/searching",
            "configuration/pr-section",
            "configuration/issue-section",
            "configuration/notification-section",
            "configuration/repo-paths",
            "configuration/keybindings",
            "configuration/theme",
            "configuration/reusing",
            "configuration/examples",
            {
              label: "Layout",
              items: [
                "configuration/layout/options",
                "configuration/layout/pr",
                "configuration/layout/issue",
              ],
            },
          ],
        },
      ],
    }),
  ],
});
