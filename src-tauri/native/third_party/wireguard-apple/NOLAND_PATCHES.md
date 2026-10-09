# Noland compatibility patch

Source: WireGuard Apple package carried by the previous No Land iOS reference.

The `WireGuardAdapter.start()` work-queue closure explicitly captures `self`.
This preserves its existing strong lifetime while avoiding Xcode 27's
`ImplicitStrongCapture` diagnostic for the nested weak path-monitor closure.
