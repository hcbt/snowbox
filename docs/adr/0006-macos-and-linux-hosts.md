# v1 Hosts are macOS and Linux

v1 is a local tool. The authoring machine is macOS; machines a later platform would use are Linux. Shipping only KVM means Snowbox does not run where the work happens. Shipping only Virtualization.framework means the platform is a rewrite.

v1 supports both as Hosts. That is two hypervisors and one UX, not a hypervisor abstraction layer as a product. macOS is first so the tool runs where the work happens; Linux KVM follows, still in v1.
