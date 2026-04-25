def normalize_vlans(vlans):
    return sorted(vlans, key=lambda vlan: vlan.vlan_id)

