#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path


NETCONF_BASE_NS = "urn:ietf:params:xml:ns:netconf:base:1.0"
H3C_CONFIG_NS = "http://www.h3c.com/netconf/config:1.0"
H3C_INTERFACE_RE = re.compile(
    r"^(?:GigabitEthernet|Ten-GigabitEthernet|FortyGigE|GE|XGE|FGE)1/0/([1-9][0-9]*)(?:\.\d+)?$"
)


def build_access_cleanup_payload(interface_name: str, pvid: int) -> str:
    ifindex = interface_ifindex(interface_name)
    pvid = validate_vlan_id(pvid, "access PVID")
    return (
        f'<config xmlns="{NETCONF_BASE_NS}">'
        f'<top xmlns="{H3C_CONFIG_NS}">'
        "<VLAN><AccessInterfaces><Interface>"
        f"<IfIndex>{ifindex}</IfIndex>"
        f"<PVID>{pvid}</PVID>"
        "</Interface></AccessInterfaces></VLAN>"
        "</top></config>"
    )


def build_trunk_cleanup_payload(interface_name: str, allowed_vlans: list[int]) -> str:
    ifindex = interface_ifindex(interface_name)
    vlans = ",".join(str(validate_vlan_id(vlan, "trunk allowed VLAN")) for vlan in allowed_vlans)
    if not vlans:
        raise ValueError("trunk allowed VLAN list must not be empty")
    return (
        f'<config xmlns="{NETCONF_BASE_NS}">'
        f'<top xmlns="{H3C_CONFIG_NS}">'
        "<VLAN><TrunkInterfaces><Interface>"
        f"<IfIndex>{ifindex}</IfIndex>"
        f"<PermitVlanList>{vlans}</PermitVlanList>"
        "</Interface></TrunkInterfaces></VLAN>"
        "</top></config>"
    )


def build_vlan_delete_payload(vlan_id: int) -> str:
    vlan_id = validate_vlan_id(vlan_id, "delete VLAN")
    return (
        f'<config xmlns="{NETCONF_BASE_NS}">'
        f'<top xmlns="{H3C_CONFIG_NS}">'
        '<VLAN><VLANs>'
        f'<VLANID xmlns:nc="{NETCONF_BASE_NS}" nc:operation="delete">'
        f"<ID>{vlan_id}</ID>"
        "</VLANID>"
        "</VLANs></VLAN>"
        "</top></config>"
    )


def build_description_cleanup_payload(
    interface_name: str,
    description: str | None,
    *,
    clear: bool,
) -> str:
    ifindex = interface_ifindex(interface_name)
    if clear:
        description_node = (
            f'<Description xmlns:nc="{NETCONF_BASE_NS}" nc:operation="delete" />'
        )
    else:
        text = "" if description is None else str(description)
        if not text:
            raise ValueError("description is required unless clear=True")
        description_node = f"<Description>{xml_escape(text)}</Description>"
    return (
        f'<config xmlns="{NETCONF_BASE_NS}">'
        f'<top xmlns="{H3C_CONFIG_NS}">'
        "<Ifmgr><Interfaces><Interface>"
        f"<IfIndex>{ifindex}</IfIndex>"
        f"{description_node}"
        "</Interface></Interfaces></Ifmgr>"
        "</top></config>"
    )


def interface_ifindex(name: str) -> int:
    match = H3C_INTERFACE_RE.fullmatch(str(name).strip())
    if match is None:
        raise ValueError(f"unsupported H3C interface name: {name}")
    return int(match.group(1))


def validate_vlan_id(value: int, field: str) -> int:
    vlan_id = int(value)
    if not 1 <= vlan_id <= 4094:
        raise ValueError(f"{field} out of range: {vlan_id}")
    return vlan_id


def parse_vlan_list(value: str) -> list[int]:
    vlans = []
    for raw in str(value).split(","):
        token = raw.strip()
        if not token:
            continue
        vlans.append(validate_vlan_id(int(token), "trunk allowed VLAN"))
    if not vlans:
        raise ValueError("trunk allowed VLAN list must not be empty")
    return vlans


def xml_escape(value: str) -> str:
    return (
        value.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
        .replace("'", "&apos;")
    )


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Restore H3C real-device acceptance test VLAN/access/trunk changes."
    )
    parser.add_argument("--host", required=True, help="Switch management IP or hostname.")
    parser.add_argument("--port", type=int, default=830, help="NETCONF SSH port.")
    parser.add_argument("--secret-ref", required=True, help="Secret reference for the NETCONF account.")
    parser.add_argument(
        "--secret-file",
        default=os.getenv("ARIA_UNDERLAY_SECRET_FILE", "/etc/aria-underlay/secrets.json"),
        help="Local JSON secret file.",
    )
    parser.add_argument("--access-interface", help="Access interface to restore.")
    parser.add_argument(
        "--access-pvid",
        type=int,
        default=1,
        help="PVID to restore on --access-interface.",
    )
    parser.add_argument("--trunk-interface", help="Trunk interface to restore.")
    parser.add_argument(
        "--trunk-allowed-vlans",
        help="Comma-separated VLAN list to restore on --trunk-interface.",
    )
    parser.add_argument(
        "--delete-vlan",
        type=int,
        action="append",
        default=[],
        help="Test VLAN ID to delete after interface restore. May be repeated.",
    )
    parser.add_argument("--description-interface", help="Interface description to restore or clear.")
    parser.add_argument("--description", help="Description text to restore.")
    parser.add_argument(
        "--clear-description",
        action="store_true",
        help="Delete --description-interface description instead of restoring text.",
    )
    parser.add_argument("--timeout", type=int, default=30, help="NETCONF connection timeout seconds.")
    parser.add_argument("--dry-run", action="store_true", help="Print payloads without connecting.")
    parser.add_argument("--yes", action="store_true", help="Required for real device writes.")
    return parser.parse_args(argv)


def build_payloads(args: argparse.Namespace) -> list[tuple[str, str]]:
    payloads = []
    if args.access_interface:
        payloads.append(
            (
                f"restore access {args.access_interface} PVID {args.access_pvid}",
                build_access_cleanup_payload(args.access_interface, args.access_pvid),
            )
        )
    if args.trunk_interface:
        if not args.trunk_allowed_vlans:
            raise SystemExit("--trunk-allowed-vlans is required with --trunk-interface")
        allowed_vlans = parse_vlan_list(args.trunk_allowed_vlans)
        payloads.append(
            (
                f"restore trunk {args.trunk_interface} allowed VLANs {args.trunk_allowed_vlans}",
                build_trunk_cleanup_payload(args.trunk_interface, allowed_vlans),
            )
        )
    if args.description_interface:
        if args.clear_description and args.description:
            raise SystemExit("--description cannot be used with --clear-description")
        if not args.clear_description and not args.description:
            raise SystemExit("--description is required unless --clear-description is set")
        label = (
            f"clear description on {args.description_interface}"
            if args.clear_description
            else f"restore description on {args.description_interface}"
        )
        payloads.append(
            (
                label,
                build_description_cleanup_payload(
                    args.description_interface,
                    args.description,
                    clear=args.clear_description,
                ),
            )
        )
    for vlan_id in args.delete_vlan:
        payloads.append((f"delete VLAN {vlan_id}", build_vlan_delete_payload(vlan_id)))
    if not payloads:
        raise SystemExit("no cleanup operation requested")
    return payloads


def validate_safety_gate(args: argparse.Namespace) -> None:
    if not args.dry_run and not args.yes:
        raise SystemExit("refusing to connect without --yes; use --dry-run to inspect payloads")


def execute_payloads(args: argparse.Namespace, payloads: list[tuple[str, str]]) -> None:
    manager, secret_provider = _load_runtime_dependencies()
    secret = secret_provider.LocalSecretProvider(args.secret_file).resolve(args.secret_ref)
    connect_args = {
        "host": args.host,
        "port": args.port,
        "username": secret.username,
        "password": secret.password,
        "key_filename": secret.key_path,
        "hostkey_verify": False,
        "look_for_keys": False,
        "allow_agent": False,
        "timeout": args.timeout,
    }
    if secret.passphrase:
        connect_args["passphrase"] = secret.passphrase

    with manager.connect(**connect_args) as session:
        for label, payload in payloads:
            print(f"applying: {label}")
            session.edit_config(
                target="running",
                config=payload,
                default_operation="merge",
                error_option="rollback-on-error",
            )


def _load_runtime_dependencies():
    repo_root = Path(__file__).resolve().parents[1]
    adapter_path = repo_root / "adapter-python"
    if adapter_path.exists():
        sys.path.insert(0, str(adapter_path))
    from ncclient import manager
    from aria_underlay_adapter import secret_provider

    return manager, secret_provider


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    validate_safety_gate(args)
    payloads = build_payloads(args)
    if args.dry_run:
        for label, payload in payloads:
            print(f"# {label}")
            print(payload)
        return 0
    execute_payloads(args, payloads)
    print("cleanup complete")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
