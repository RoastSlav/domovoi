import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

/** Local rules that keep page code on the design system. */
const domovoi = {
  rules: {
    "no-native-title": {
      meta: {
        type: "suggestion",
        schema: [],
        messages: {
          title:
            "Native title tooltips are unreliable on touch and keyboard; use Tooltip, InfoTip or visible text (web/DESIGN.md).",
        },
      },
      create(context) {
        return {
          JSXAttribute(node) {
            const element = node.parent.name;
            const dom = element.type === "JSXIdentifier" && /^[a-z]/.test(element.name);
            if (dom && node.name.name === "title") {
              context.report({ node, messageId: "title" });
            }
          },
        };
      },
    },
  },
};

export default tseslint.config(
  { ignores: ["dist", "src/api/schema.d.ts"] },
  {
    files: ["**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.strict],
    languageOptions: {
      ecmaVersion: 2023,
      globals: globals.browser,
    },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      // ADR 0006: only src/api talks to the network, through the typed client.
      "no-restricted-globals": [
        "error",
        { name: "fetch", message: "Use the typed client in src/api (ADR 0006)." },
      ],
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["**/api/schema"],
              message: "Import generated types only inside src/api; expose hooks instead.",
            },
          ],
        },
      ],
    },
  },
  {
    // Long files hide design drift; split a file before it passes the limit.
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/**/*.test.{ts,tsx}", "src/test/**"],
    rules: {
      "max-lines": ["error", { max: 250, skipBlankLines: true, skipComments: true }],
    },
  },
  {
    // The one table of every status in the product: a data file, kept whole on purpose.
    files: ["src/design/tone.ts"],
    rules: { "max-lines": ["error", { max: 540, skipBlankLines: true, skipComments: true }] },
  },
  {
    // Page code composes the kit; it does not style or explain things by hand (web/DESIGN.md).
    files: ["src/features/**/*.tsx", "src/app/**/*.tsx"],
    ignores: ["src/**/*.test.tsx"],
    plugins: { domovoi },
    rules: {
      "no-restricted-syntax": [
        "error",
        {
          selector: "JSXAttribute[name.name='style']",
          message:
            "No inline styles in page code: use a CSS module with tokens. For a value computed at run time, disable this line with a comment that says why.",
        },
      ],
      "domovoi/no-native-title": "error",
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["**/api/schema"],
              message: "Import generated types only inside src/api; expose hooks instead.",
            },
            {
              regex: "/components/(?!ui\\.module\\.css$).+",
              message: "Import kit components from the barrel: src/components (web/DESIGN.md).",
            },
            {
              group: ["lucide-react"],
              message: "Draw icons through Icon in src/design/icons.tsx.",
            },
          ],
        },
      ],
    },
  },
  {
    files: ["src/api/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-globals": "off",
      "no-restricted-imports": "off",
    },
  },
);
