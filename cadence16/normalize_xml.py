#!/usr/bin/env python3
"""Normalize npnp's Library XML for OrCAD Capture 16.6 XML2OLB.

16.6's importer is stricter than 17.x:
- <LibPart> carries a CellName attribute 16.6 rejects -> strip
- timestamp/timezone/charset/ValueIfValueExist attributes -> strip
- <Ellipse> elements -> remove (16.6 XML2OLB chokes on them)
Gate numbering attributes (alphabeticNumbering / isHomogeneous) pass
through unchanged: the cadence-16.6 branch already emits the values 16.6
expects (single-gate 2/1, heterogeneous 1/0).

Usage: python normalize_xml.py <in.xml> <out.xml>
"""
import re
import sys

def normalize(text: str) -> str:
    text = re.sub(r'<LibPart>\s*<Defn CellName="[^"]*"/>',
                  '<LibPart>\n      <Defn/>', text)
    text = re.sub(r' timestamp="\d+" timezone="\d+"', '', text)
    text = re.sub(r' charset="\d+"', '', text)
    text = re.sub(r' ValueIfValueExist="\d+"', '', text)
    text = re.sub(r'<Ellipse>.*?</Ellipse>\s*', '', text, flags=re.S)
    return text

def main() -> None:
    if len(sys.argv) != 3:
        sys.exit('usage: normalize_xml.py <in.xml> <out.xml>')
    data = open(sys.argv[1], encoding='utf-8').read()
    out = normalize(data)
    open(sys.argv[2], 'w', encoding='utf-8', newline='\n').write(out)
    print('normalized:', sys.argv[2])

if __name__ == '__main__':
    main()
