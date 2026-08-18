# Studio key router

Exclusive pane focus: **Workflow | Tree | View | EditSpec**.
One key router owns Overlay → EditSpec Select/EditRegion → focused-pane Idle → session chrome.
Session idle is overlay None AND pane mode Idle. Overlay depth is 0 or 1; Help and Run Composer never stack.

| Keys | Action |
|---|---|
| `Tab` / `Shift+Tab` | Idle: cycle panes. Select: exit to Idle (keep selection) then cycle. EditRegion: form field. Overlay: freeze |
| `f` | Collapse / expand the focused pane (idle only); click a pane's top-right `▾`/`▸` to fold that pane |
| `q` / `Ctrl+C` | Quit only when idle (not overlay / form) |
| `?` | Help overlay; exclusive with Composer |
| `h/l j/k w/a/s/d u/i [ ] v` | 3D camera / viz in View Idle; `j/k` rotate only when View is focused |
| `x` | Enter Select (EditSpec Idle only) |
| `Enter` | EditSpec Idle: open EditRegion form. Tree: load structure. Workflow / View: Ignore. Select: operate on the range |
| `e` | Edit the focused existing region (EditSpec only) |
| `Ctrl+R` | Open Run Composer from any pane including Select; Ignore if Composer/Status is open or EditRegion form is open |
| Select: `h/l H/L s [ ] 1-5` | Sequence cursor / boundary / segment / action shortcuts |
| Select: `Esc` | Clear selection and return EditSpec Idle (one Esc; no two-Esc contract) |
| EditRegion | Form keys only (`A51-80` / `A:51-80` / `51-80`); Tab is a field; Esc cancels the form |
| Run Composer: `Esc` | Close overlay; Ctrl+R does not stack |

Mouse: hit-test uses the inner rect (outer chrome minus 1 col/row). Letter line clicks that residue and starts a drag; secondary-structure line selects the segment (same as `s`). Action-marker line is Ignore. View left-drag rotates; View wheel zooms; EditSpec wheel scrolls. Drag a shared pane border to resize. Tree: `▾`/`▸` toggles children only; label selects and loads View/EditSpec from `structure_path` or the first descendant structure. Wheel scrolls the tree. One Line = one row. View-focused clicks do not enter Select from the sequence. Click another pane: Select → Idle (keep selection); EditRegion → cancel form and Idle; then that pane's Idle/click.

PDB launch (`gemlib studio` / `edit`) opens View + EditSpec. Tree and Workflow start collapsed. A campaign directory lands on Tree. Workflow graph drawing is a later slice.
