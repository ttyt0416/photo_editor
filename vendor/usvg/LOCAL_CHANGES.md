# Local usvg 0.47.0 patch

Source: the published usvg 0.47.0 crate, https://github.com/linebender/resvg. Upstream MIT and Apache-2.0 licenses are retained.

`parser/converter.rs` adds a helper accepting an already resolved transform; the original entry point keeps its behavior. `parser/use_node.rs` uses identity for group resolution after applying the complete use/nested-SVG transform to parent coordinates, preventing duplicate absolute transforms. Symbols retain the outer use transform, apply only the viewport transform inside it, and clipped-use group absolute coordinates include the outer transform. XML, CSS, reference nodes, and native output are unchanged.

Regression coverage is in the application's `svg_objects` tests, including transformed shapes/groups, use instances and symbols, viewport growth, masks, and native outputs. Recheck this patch when updating usvg/resvg; it is not an upstream release claim.
