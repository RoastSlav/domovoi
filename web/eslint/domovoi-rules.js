/**
 * Local ESLint rules that keep page code on the design system (see web/DESIGN.md,
 * "Checks that will fail your PR"). Each rule has one message that says what to do instead.
 */

/** Verdict names. Their label, tone and hint live in src/design/tone.ts; lowercase API values are data and stay free. */
const STATUS_LITERALS = new Set(["ACTION_NEEDED", "MONITOR", "EXPECTED", "FIXED"]);

/** Names whose import shows a file reports what its mutations did. */
const FEEDBACK_NAMES = new Set(["useActionFeedback", "InlineMessage", "InlineError"]);

/** Written in a comment, this vouches that a mutation's result is reported elsewhere. */
const FEEDBACK_ESCAPE = /feedback-ok:\s*\S+/;

const MUTATION_CALLS = new Set(["mutate", "mutateAsync"]);

const rules = {
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

  "no-raw-button": {
    meta: {
      type: "suggestion",
      schema: [],
      messages: {
        button:
          "Use Button or IconButton from the kit: they carry the focus ring, the loading state and the disabled reason (web/DESIGN.md).",
      },
    },
    create(context) {
      return {
        JSXOpeningElement(node) {
          if (node.name.type === "JSXIdentifier" && node.name.name === "button") {
            context.report({ node, messageId: "button" });
          }
        },
      };
    },
  },

  "no-status-literal": {
    meta: {
      type: "suggestion",
      schema: [],
      messages: {
        status:
          "Do not decide a status's label or tone here. Use the lookup in src/design/tone.ts and render it with StatusPill (web/DESIGN.md).",
      },
    },
    create(context) {
      return {
        Literal(node) {
          if (typeof node.value === "string" && STATUS_LITERALS.has(node.value)) {
            context.report({ node, messageId: "status" });
          }
        },
        JSXText(node) {
          if (STATUS_LITERALS.has(node.value.trim())) {
            context.report({ node, messageId: "status" });
          }
        },
      };
    },
  },

  "disabled-needs-reason": {
    meta: {
      type: "suggestion",
      schema: [],
      messages: {
        reason:
          "A control that cannot be used says why: pass disabledReason (Button, IconButton, Checkbox) instead of disabled. If a Field hint already says why, disable this line with a comment that names it.",
      },
    },
    create(context) {
      return {
        JSXOpeningElement(node) {
          const name = node.name.type === "JSXIdentifier" ? node.name.name : "";
          if (name === "option" || name === "optgroup") {
            return;
          }
          const names = node.attributes.map((a) => (a.type === "JSXAttribute" ? a.name.name : ""));
          const disabled = node.attributes.find(
            (a) => a.type === "JSXAttribute" && a.name.name === "disabled",
          );
          if (disabled && !names.includes("disabledReason")) {
            context.report({ node: disabled, messageId: "reason" });
          }
        },
      };
    },
  },

  "mutation-feedback": {
    meta: {
      type: "problem",
      schema: [],
      messages: {
        silent:
          "A mutation must report its result. Wrap it in useActionFeedback, or show its error with InlineMessage or InlineError. If it is reported elsewhere, add a comment `feedback-ok: <where>` to this file (web/DESIGN.md).",
      },
    },
    create(context) {
      const source = context.sourceCode;
      let reported = false;
      const vouched = source.getAllComments().some((c) => FEEDBACK_ESCAPE.test(c.value));
      return {
        ImportSpecifier(node) {
          if (FEEDBACK_NAMES.has(node.imported.name)) {
            reported = true;
          }
        },
        CallExpression(node) {
          const callee = node.callee;
          if (
            callee.type === "MemberExpression" &&
            callee.property.type === "Identifier" &&
            MUTATION_CALLS.has(callee.property.name) &&
            !reported &&
            !vouched
          ) {
            context.report({ node, messageId: "silent" });
          }
        },
      };
    },
  },
};

/** The plugin object ESLint's config registers as `domovoi`. */
export default { rules };
