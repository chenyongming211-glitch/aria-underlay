"""Device configuration sample collector with automatic sanitization.

Collects NETCONF running-config XML from devices and automatically sanitizes
sensitive information (IPs, passwords, community strings, AS numbers) while
preserving XML structure for parser validation and LLM-assisted adaptation.
"""
from __future__ import annotations

import re
import sys
import hashlib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any
from xml.etree import ElementTree


@dataclass
class SanitizationReport:
    """Report of sanitization actions performed."""

    ip_addresses_replaced: int = 0
    passwords_redacted: int = 0
    community_strings_redacted: int = 0
    as_numbers_anonymized: int = 0
    device_names_anonymized: int = 0
    other_sensitive_fields: int = 0
    total_elements: int = 0

    def summary(self) -> str:
        """Generate human-readable summary."""
        return (
            f"Sanitization report:\n"
            f"  Total XML elements: {self.total_elements}\n"
            f"  IP addresses replaced: {self.ip_addresses_replaced}\n"
            f"  Passwords redacted: {self.passwords_redacted}\n"
            f"  Community strings redacted: {self.community_strings_redacted}\n"
            f"  AS numbers anonymized: {self.as_numbers_anonymized}\n"
            f"  Device names anonymized: {self.device_names_anonymized}\n"
            f"  Other sensitive fields: {self.other_sensitive_fields}\n"
            f"  Total sensitive items: {self.total_redacted_items()}"
        )

    def total_redacted_items(self) -> int:
        """Total number of sensitive items redacted."""
        return (
            self.ip_addresses_replaced
            + self.passwords_redacted
            + self.community_strings_redacted
            + self.as_numbers_anonymized
            + self.device_names_anonymized
            + self.other_sensitive_fields
        )


# RFC 5737 documentation addresses for IP replacement
_DOCUMENTATION_IPS = [
    "192.0.2.",  # TEST-NET-1
    "198.51.100.",  # TEST-NET-2
    "203.0.113.",  # TEST-NET-3
]

# Element names that typically contain passwords
_PASSWORD_ELEMENTS = {
    "Password",
    "password",
    "Passphrase",
    "passphrase",
    "AuthPassword",
    "EncryptedPassword",
    "Secret",
    "secret",
    "Key",
    "AuthenticationKey",
}

# Element names that typically contain AS numbers
_AS_NUMBER_ELEMENTS = {
    "ASNumber",
    "LocalAS",
    "RemoteAS",
    "AS",
    "BGPAS",
    "AutonomousSystem",
}

# Element names that typically contain device names
_DEVICE_NAME_ELEMENTS = {
    "Hostname",
    "hostname",
    "DeviceName",
    "SysName",
    "SystemName",
    "DeviceID",
}

# IPv4 regex pattern
_IPV4_PATTERN = re.compile(
    r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}"
    r"(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b"
)


@dataclass
class SampleCollector:
    """Collects and sanitizes device configuration samples."""

    # IP address mapping cache to ensure consistent replacement
    _ip_mapping: dict[str, str] = field(default_factory=dict)
    _ip_counter: int = 0

    def _replace_ip(self, ip: str) -> str:
        """Replace an IP address with a documentation address."""
        if ip in self._ip_mapping:
            return self._ip_mapping[ip]

        # Use round-robin across documentation ranges
        prefix = _DOCUMENTATION_IPS[self._ip_counter % len(_DOCUMENTATION_IPS)]
        new_ip = f"{prefix}{(self._ip_counter % 254) + 1}"
        self._ip_mapping[ip] = new_ip
        self._ip_counter += 1
        return new_ip

    def _anonymize_as_number(self, as_number: str) -> str:
        """Anonymize an AS number using hash-based replacement."""
        try:
            as_int = int(as_number)
            # Hash the AS number to get a deterministic replacement
            hash_val = int(hashlib.md5(str(as_int).encode()).hexdigest()[:8], 16)
            # Map to private AS range: 64512-65534 or 4200000000-4294967294
            if as_int < 65536:
                new_as = 64512 + (hash_val % 1022)
            else:
                new_as = 4200000000 + (hash_val % 94967294)
            return str(new_as)
        except ValueError:
            return as_number

    def _anonymize_device_name(self, name: str) -> str:
        """Anonymize a device name."""
        # Simple hash-based anonymization
        hash_val = int(hashlib.md5(name.encode()).hexdigest()[:6], 16)
        return f"device-{hash_val:04d}"

    def _sanitize_element(
        self, element: ElementTree.Element, report: SanitizationReport
    ) -> None:
        """Recursively sanitize an XML element."""
        tag_local = element.tag.split("}")[-1] if "}" in element.tag else element.tag

        # Check if this is a password element
        if tag_local in _PASSWORD_ELEMENTS:
            if element.text and element.text.strip():
                element.text = "[REDACTED]"
                report.passwords_redacted += 1

        # Check if this is an AS number element
        elif tag_local in _AS_NUMBER_ELEMENTS:
            if element.text and element.text.strip():
                original = element.text.strip()
                element.text = self._anonymize_as_number(original)
                if element.text != original:
                    report.as_numbers_anonymized += 1

        # Check if this is a device name element
        elif tag_local in _DEVICE_NAME_ELEMENTS:
            if element.text and element.text.strip():
                original = element.text.strip()
                element.text = self._anonymize_device_name(original)
                if element.text != original:
                    report.device_names_anonymized += 1

        # Check for community strings (case-insensitive)
        elif "community" in tag_local.lower():
            if element.text and element.text.strip():
                element.text = "[REDACTED]"
                report.community_strings_redacted += 1

        # Sanitize text content for IP addresses
        if element.text:
            ips_found = _IPV4_PATTERN.findall(element.text)
            if ips_found:
                for ip in ips_found:
                    new_ip = self._replace_ip(ip)
                    element.text = element.text.replace(ip, new_ip)
                report.ip_addresses_replaced += len(ips_found)

        # Recursively process children
        for child in element:
            self._sanitize_element(child, report)

    def sanitize_xml(self, xml_content: str) -> tuple[str, SanitizationReport]:
        """Sanitize XML content and return sanitized version with report.

        Args:
            xml_content: Raw XML content from device

        Returns:
            Tuple of (sanitized_xml, sanitization_report)
        """
        report = SanitizationReport()

        try:
            root = ElementTree.fromstring(xml_content)
        except ElementTree.ParseError as e:
            raise ValueError(f"Failed to parse XML: {e}") from e

        # Count total elements
        report.total_elements = sum(1 for _ in root.iter())

        # Sanitize the tree
        self._sanitize_element(root, report)

        # Convert back to string
        sanitized_xml = ElementTree.tostring(
            root, encoding="unicode", xml_declaration=True
        )

        return sanitized_xml, report


def collect_and_sanitize_sample(
    device_ip: str,
    device_port: int,
    username: str,
    password: str,
    output_path: Path,
    *,
    hostkey_verify: bool = False,
) -> SanitizationReport:
    """Collect device configuration and save sanitized sample.

    Args:
        device_ip: Device management IP
        device_port: NETCONF port (usually 830)
        username: NETCONF username
        password: NETCONF password
        output_path: Where to save sanitized XML
        hostkey_verify: Whether to verify SSH host key

    Returns:
        Sanitization report
    """
    try:
        from ncclient import manager
    except ImportError as e:
        raise RuntimeError(
            "ncclient is required for device collection. "
            "Install with: pip install ncclient"
        ) from e

    print(f"Connecting to {device_ip}:{device_port}...")
    with manager.connect(
        host=device_ip,
        port=device_port,
        username=username,
        password=password,
        hostkey_verify=hostkey_verify,
        timeout=30,
    ) as session:
        print("Connected. Retrieving running configuration...")
        config = session.get_config(source="running")
        raw_xml = config.data_xml

        print("Sanitizing sensitive information...")
        collector = SampleCollector()
        sanitized_xml, report = collector.sanitize_xml(raw_xml)

        # Save sanitized XML
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(sanitized_xml, encoding="utf-8")

        print(f"\nSaved sanitized sample to: {output_path}")
        print(report.summary())

        # Save report as JSON
        report_path = output_path.with_suffix(".report.json")
        import json

        report_data = {
            "output_file": str(output_path),
            "total_elements": report.total_elements,
            "ip_addresses_replaced": report.ip_addresses_replaced,
            "passwords_redacted": report.passwords_redacted,
            "community_strings_redacted": report.community_strings_redacted,
            "as_numbers_anonymized": report.as_numbers_anonymized,
            "device_names_anonymized": report.device_names_anonymized,
            "other_sensitive_fields": report.other_sensitive_fields,
            "total_redacted_items": report.total_redacted_items(),
        }
        report_path.write_text(json.dumps(report_data, indent=2), encoding="utf-8")
        print(f"Saved sanitization report to: {report_path}")

        return report


def main() -> int:
    """CLI entry point for sample collector."""
    import argparse

    parser = argparse.ArgumentParser(
        description="Collect and sanitize device configuration samples",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Collect from H3C device
  collect-device-sample --device 10.0.0.1 --user admin --output h3c-sample.xml

  # With custom port
  collect-device-sample --device 10.0.0.1 --port 830 --user admin --output sample.xml

  # From saved raw XML (no device connection)
  collect-device-sample --from-file raw.xml --output sanitized.xml
        """,
    )

    parser.add_argument(
        "--device",
        help="Device IP address or hostname",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=830,
        help="NETCONF port (default: 830)",
    )
    parser.add_argument(
        "--user",
        help="NETCONF username",
    )
    parser.add_argument(
        "--password",
        help="NETCONF password (will prompt if not provided)",
    )
    parser.add_argument(
        "--output",
        required=True,
        type=Path,
        help="Output path for sanitized XML",
    )
    parser.add_argument(
        "--from-file",
        type=Path,
        help="Sanitize existing XML file instead of connecting to device",
    )
    parser.add_argument(
        "--verify-hostkey",
        action="store_true",
        help="Verify SSH host key (default: skip verification)",
    )

    args = parser.parse_args()

    # Mode 1: Sanitize existing file
    if args.from_file:
        if not args.from_file.exists():
            print(f"Error: Input file not found: {args.from_file}", file=sys.stderr)
            return 1

        print(f"Reading raw XML from: {args.from_file}")
        raw_xml = args.from_file.read_text(encoding="utf-8")

        print("Sanitizing sensitive information...")
        collector = SampleCollector()
        sanitized_xml, report = collector.sanitize_xml(raw_xml)

        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(sanitized_xml, encoding="utf-8")

        print(f"\nSaved sanitized sample to: {args.output}")
        print(report.summary())

        # Save report as JSON (parity with collect_and_sanitize_sample path)
        report_path = args.output.with_suffix(".report.json")
        import json

        report_data = {
            "output_file": str(args.output),
            "total_elements": report.total_elements,
            "ip_addresses_replaced": report.ip_addresses_replaced,
            "passwords_redacted": report.passwords_redacted,
            "community_strings_redacted": report.community_strings_redacted,
            "as_numbers_anonymized": report.as_numbers_anonymized,
            "device_names_anonymized": report.device_names_anonymized,
            "other_sensitive_fields": report.other_sensitive_fields,
            "total_redacted_items": report.total_redacted_items(),
        }
        report_path.write_text(json.dumps(report_data, indent=2), encoding="utf-8")
        print(f"Saved sanitization report to: {report_path}")

        return 0

    # Mode 2: Collect from device
    if not args.device or not args.user:
        parser.error("--device and --user are required when not using --from-file")

    password = args.password
    if not password:
        import getpass

        password = getpass.getpass(f"Password for {args.user}@{args.device}: ")

    try:
        collect_and_sanitize_sample(
            device_ip=args.device,
            device_port=args.port,
            username=args.user,
            password=password,
            output_path=args.output,
            hostkey_verify=args.verify_hostkey,
        )
        return 0
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
