---
name: designbridge
description: Use DesignBridge MCP links to inspect locally captured Lanhu designs and implement a selected UI module in the current business project.
---

# DesignBridge UI Workflow

Use this skill when the user provides a `designbridge://design/...` link, asks to implement UI from DesignBridge, or asks for Lanhu layers, slices, styles, screenshots, or comments.

## Keep the two projects separate

The current working directory is the business project that Codex should modify. DesignBridge is a separate desktop tool and MCP provider. Do not copy DesignBridge source into the business project and do not change the business project's MCP configuration.

## Read the design in a focused order

1. Call `resolve_design` with the supplied link. Confirm the design frame, coordinate space, and whether the link contains a selected `node-id` or rectangle.
2. Call `get_design_context` with the same link. For a module link, inspect the selected container and its descendants before writing code. Use a focused `maxDepth` when the subtree is large.
3. Call `get_comments` for the design. Pay attention to comments whose coordinates fall inside or near the selected module; treat their content as implementation requirements.
4. Call `get_assets` only for the selected module's slice layers and only after the relevant layer ids are known. Save requested images into the business project's existing asset directory and preserve the required format or scale.
5. Use `get_design_screenshot` for visual comparison when the user supplied a screenshot or when the module boundary needs confirmation.

Do not query the entire design tree when a selected module link is available. Do not request Lanhu directly from TypeScript or the browser; DesignBridge performs network access and image processing in Rust.

## Implement the module

- Reuse the business project's existing framework, component patterns, tokens, and asset conventions.
- Match the selected module's frame, spacing, colors, typography, borders, radius, opacity, shadows, and text runs from MCP data.
- Include child layers that belong to the selected container, but do not implement unrelated page content.
- Use retrieved slices instead of recreating icons or images when a matching slice exists.
- Apply nearby Lanhu comments explicitly. Mention any comment that conflicts with existing project conventions before choosing a compromise.
- Verify the result in the business project with its normal development or test workflow.

## Link behavior

- A link without `node-id` identifies the whole design.
- A link with `?node-id=...` identifies a module or layer and should be treated as the implementation scope.
- If no node is selected, ask the user to select the module's outermost container in DesignBridge, copy its MCP link, and send it again with the target screenshot.
