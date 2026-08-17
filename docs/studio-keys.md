# Studio key router

Exclusive pane focus: **Workflow | Tree | View | EditSpec**.
`Tab` / `Shift+Tab` cycle focus. `f` collapses or expands the focused pane.

Interaction modes: **View | Select | EditRegion | Run**. One router owns all keys.

| Keys | Action |
|---|---|
| `Tab` / `Shift+Tab` | Cycle exclusive pane focus |
| `f` | Collapse / expand the focused pane |
| `q` | Quit (View mode only) |
| `h/l j/k w/a/s/d u/i [ ] v` | 3D camera / viz in View; `j/k` rotate only when View is focused |
| `x` | Enter Select |
| `Enter` | Open EditRegion (empty, or prefilled from an active selection) |
| `e` | Edit the focused existing region |
| `Ctrl+R` | Open Run overlay from View only; ignored in other modes |
| Select: `h/l H/L s [ ] 1-5` | Sequence cursor / boundary / segment / action shortcuts |
| Select: `Esc` | Return to View (selection remains until a second Esc) |
| EditRegion | Form keys only (`A51-80` / `A:51-80` / `51-80`); 3D keys disabled; Esc restores previous mode |
| Run: `Esc` | Close overlay; `Ctrl+R` does not stack |

PDB launch (`gemlib studio` / `edit`) opens View + EditSpec. Tree and Workflow stay collapsed empty shells.
