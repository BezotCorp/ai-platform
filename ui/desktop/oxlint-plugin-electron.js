const noWindowLocationHref = {
  meta: {
    type: "problem",
    docs: {
      description:
        "Disallow direct usage of window.location.href in Electron apps",
    },
  },

  create(context) {
    return {
      AssignmentExpression(node) {
        if (
          node.left.type === "MemberExpression" &&
          node.left.object?.type === "MemberExpression" &&
          node.left.object.object?.type === "Identifier" &&
          node.left.object.object.name === "window" &&
          node.left.object.property?.type === "Identifier" &&
          node.left.object.property.name === "location" &&
          node.left.property?.type === "Identifier" &&
          node.left.property.name === "href"
        ) {
          context.report({
            node,
            message:
              "Do not use window.location.href directly in Electron apps. " +
              "Use setView from the project instead.",
          });
        }
      },
    };
  },
};

export default {
  meta: {
    name: "electron-project",
  },

  rules: {
    "no-window-location-href": noWindowLocationHref,
  },
};
