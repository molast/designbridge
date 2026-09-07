Implement the requested UI module in the current business project using the supplied DesignBridge link and screenshot.

Workflow:

1. Resolve the `designbridge://` link.
2. Read the selected node and its descendant layers with `get_design_context`.
3. Read comments for the design and identify comments inside or near the selected module.
4. Retrieve only the linked slices needed by the module with `get_assets`.
5. Implement the module using the current project's framework and conventions.
6. Verify the implementation and report any design data that was unavailable.

The selected node is the scope. Do not implement unrelated parts of the design. Treat Lanhu comments as requirements, not optional notes.
