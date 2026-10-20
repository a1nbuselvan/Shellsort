#!/usr/bin/env python3
"""Check website-local links and fragments without third-party packages."""

from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlsplit


SITE = Path(__file__).parent / "public"
PAGE = SITE / "index.html"


class PageCheck(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.ids = set()
        self.links = []
        self.errors = []

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        element_id = attrs.get("id")
        if element_id:
            if element_id in self.ids:
                self.errors.append(f"duplicate id: #{element_id}")
            self.ids.add(element_id)

        for attribute in ("href", "src"):
            value = attrs.get(attribute)
            if value:
                self.links.append((tag, attribute, value))


def main():
    if not PAGE.is_file():
        raise SystemExit(f"missing site entrypoint: {PAGE}")

    parser = PageCheck()
    parser.feed(PAGE.read_text(encoding="utf-8"))
    parser.close()

    for tag, attribute, value in parser.links:
        parts = urlsplit(value)
        if parts.scheme or parts.netloc or value.startswith("data:"):
            continue
        if parts.path.startswith("/"):
            parser.errors.append(f"root-relative {attribute} breaks the /Shellsort/ project path: {value}")
            continue
        if parts.path:
            target = (PAGE.parent / parts.path).resolve()
            if not target.is_relative_to(SITE.resolve()):
                parser.errors.append(f"{attribute} escapes the published site: {value}")
            else:
                if target.is_dir():
                    target = target / "index.html"
                if not target.is_file():
                    parser.errors.append(f"missing local {attribute} target: {value}")
        if parts.fragment and parts.fragment not in parser.ids:
            parser.errors.append(f"missing local fragment target: #{parts.fragment}")

    if parser.errors:
        raise SystemExit("\n".join(parser.errors))

    print(
        f"Website local paths and fragments OK ({len(parser.links)} references parsed; "
        "external URLs are not fetched)."
    )


if __name__ == "__main__":
    main()
