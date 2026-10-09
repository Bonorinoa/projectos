## Variant: Workshop Console

### Design stance
Observability is the product: the registry and its live trace are co-equal panes. Denials and failures are first-class traces (colored, kept open), never hidden errors. Built for the engineering/measurement workflow.

### Key choices
- Layout: fixed two-pane grid — registry (filterable rows) + trace stream for the selection
- Typography: ui-sans body, serif titles, mono for stamps/hashes/denials
- Color: dark workshop chrome (`#1b2123` panels), moss-green accept, coral deny, amber warning
- Interaction: type filter buttons, row selection swaps the trace panel, deny-blocks expand inline

### Trade-offs
- Strong at: density, keyboard-adjacent scanning, evidence culture (receipts, denials, cost, `OUTCOME_UNKNOWN` visibly recorded)
- Weak at: seductive for the wrong audience — feels like a tool; a Substack writer may find the workshop too loud

### Best for
- The Software vertical and Academic workflows: many projects, frequent runs, audit-minded user