---
name: designbridge
description: Use DesignBridge MCP links to inspect locally captured Lanhu designs and implement a selected UI module in the current business project.
---

# DesignBridge UI Workflow

Use this skill when the user provides a `designbridge://design/...` link, asks to implement UI from DesignBridge, or asks for Lanhu layers, slices, styles, screenshots, or comments.

## Keep the two projects separate

The current working directory is the business project that Codex should modify. DesignBridge is a separate desktop tool and MCP provider. Do not copy DesignBridge source into the business project and do not change the business project's MCP configuration.

## Locate the target visually, then read its exact data

The required workflow is **link -> target screenshot -> visual location -> exact layer data**. Do not start implementation from layer metadata alone.

1. Call `resolve_design` with the supplied link. Confirm the design frame, coordinate space, and whether the link contains a selected `node-id` or rectangle.
2. Immediately call `get_design_screenshot` with the same link, before calling `get_design_context`. The tool returns the captured design image and its pixel dimensions. If the link contains a `node-id` or rectangle, use that automatic crop as the target screenshot; otherwise use the full-page screenshot.
3. Treat the returned screenshot as the visual source of truth. Inspect it to identify the target UI region, its boundary, relative position, and surrounding landmarks. When the user also supplied a screenshot, compare both images and use the user's screenshot to disambiguate the intended region.
4. Locate that region in design coordinates. Prefer the selected node's frame when the link has `node-id`; for a whole-page link, infer a rectangle from the screenshot and call `find_layers` with that `rect` (or with a distinctive text/point when available). If the visual target cannot be located confidently, ask for a tighter screenshot or a link containing the target layer instead of guessing.
5. Call `get_design_context` using the located `node-id` or focused `rect`. Read the target container and its descendants, including exact frames, hierarchy, text runs, colors, typography, borders, radius, opacity, shadows, visibility, and coordinate relationships. Use a focused `maxDepth` and `limit` when the subtree is large.
6. Call `get_comments` with the same target `node-id` or rectangle. Pay attention to comments whose coordinates fall inside or near the selected module; treat their content as implementation requirements.
7. Call `get_assets` only after the target layers are known, and request slices linked to those layer ids. Save requested images into the business project's existing asset directory and preserve the required format or scale.
8. Before writing code, cross-check the exact layer data against the target screenshot. If the crop, layer boundary, or visual landmarks do not match, repeat the screenshot/location step with a narrower rectangle; do not implement an unverified region.

Do not query the entire design tree when a selected module link is available. Do not request Lanhu directly from TypeScript or the browser; DesignBridge performs network access and image processing in Rust.

## Implement the module

- Reuse the business project's existing framework, component patterns, tokens, and asset conventions.
- Match the selected module's frame, spacing, colors, typography, borders, radius, opacity, shadows, and text runs from MCP data.
- Include child layers that belong to the selected container, but do not implement unrelated page content.
- Use retrieved slices instead of recreating icons or images when a matching slice exists.
- Apply nearby Lanhu comments explicitly. Mention any comment that conflicts with existing project conventions before choosing a compromise.
- Verify the result in the business project with its normal development or test workflow.

The screenshot is used to locate and visually validate the target; MCP layer data is used for exact measurements and implementation. Do not use the screenshot as a replacement for frames, typography, colors, comments, or assets returned by MCP.

## Link behavior

- A link without `node-id` identifies the whole design.
- A link with `?node-id=...` identifies a module or layer and should be treated as the initial implementation scope, but still capture and inspect its screenshot before reading descendants.
- If no node is selected, use the full-page screenshot plus visual landmarks to locate the target and then narrow the query with a rectangle. Ask the user to select the module's outermost container and resend its link only when the target cannot be uniquely identified from the screenshot.
