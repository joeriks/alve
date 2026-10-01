# Explore and organize the memory graph

Open **Menu → Graph**. Select a memory to read its heading, concise details and tags in the adjacent panel. Select an arrow or a relation in the panel to inspect its direction and type. Open the full memory to read exact facts and sources or resolve conflicting versions.

- **Focus here** shows the selected memory and its immediate neighbors. Expand or collapse the neighborhood, or return to the whole map.
- Search in the header and filter by tag, relation type or archived status. Filters combine with neighborhood focus and hidden memories.
- **Hide from map** changes only the current view. Show individual hidden memories in the panel or use **Graph menu → Show all hidden**. Hiding does not change storage or AI permissions.
- Drag memories to arrange them; drag the background to pan. Use +, − and Fit, or Ctrl/Command with the mouse wheel to zoom. A keyboard-accessible memory list appears below the map.
- Use **Graph menu → Select several memories**, then select memories by tapping, clicking or using the list. Create a named project group with `belongs_to` relations. Ctrl/Command-click also supports multiple selection.
- Add memories and relations from Graph menu. Edit the selected memory or relation in the panel. Quick memory editing preserves existing facts and references; the full editor can change these.
- Archive a memory to remove it from active views. Show archived memories to restore it. Archiving preserves its data, relations and history. Remove a relation to unlink its memories; **Undo unlink** restores a live link.

Relation editing creates or reuses a live replacement and retains the old relation as a tombstone in one durable operation. A stale edit is rejected. Reimporting an old bundle cannot resurrect the replaced link. Owner authorization is required; AI grants cannot directly edit relations.

The map initially renders up to 80 matching memories and explicitly reports the count. **Show more** loads another 80. Layout, hidden memories and selection are session-local, survive switching views, and are cleared when the vault locks. They are not included in backups or synchronization. Large dense graphs can still be difficult to read; focus and filters are the primary tools for exploring them.
