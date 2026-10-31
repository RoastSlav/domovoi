import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

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
    files: ["src/api/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-globals": "off",
      "no-restricted-imports": "off",
    },
  },
);
