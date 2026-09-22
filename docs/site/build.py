#!/usr/bin/env python3
import os
import re
import sys

DOCS_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SITE_DIR = os.path.dirname(os.path.abspath(__file__))


def escape_html(text):
    return text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def inline_format(text):
    text = escape_html(text)
    text = re.sub(r'\*\*([^*]+)\*\*', r'<strong>\1</strong>', text)
    text = re.sub(r'\`([^\`]+)\`', r'<code>\1</code>', text)
    return text


def process_inline_links(text):
    result = []
    i = 0
    while i < len(text):
        m = re.search(r'\[([^\]]+)\]\(([^)]+)\)', text[i:])
        if not m:
            result.append(inline_format(text[i:]))
            break
        start = i + m.start()
        end = i + m.end()
        result.append(inline_format(text[i:start]))
        link_text = m.group(1)
        link_url = m.group(2)
        link_text_processed = inline_format(link_text)
        link_url = re.sub(r'\.md(#.+)?$', r'.html\1', link_url)
        link_url = re.sub(r'^docs/', '', link_url)
        result.append(f'<a href="{escape_html(link_url)}">{link_text_processed}</a>')
        i = end
    return "".join(result)


def convert_md(filepath):
    with open(filepath, "r", encoding="utf-8") as f:
        lines = f.readlines()

    out = []
    in_code = False
    in_list = False
    in_table = False
    code_lang = ""

    for raw_line in lines:
        line = raw_line.rstrip("\n")

        if in_code:
            if line.strip() == "```":
                out.append("</code></pre>")
                in_code = False
            else:
                out.append(escape_html(line))
            continue

        m = re.match(r"^\s*```(nct|text|bash|rust|c|python|js|ts)\s*$", line)
        if m:
            code_lang = m.group(1)
            out.append(f'<pre><code class="language-{code_lang}">')
            in_code = True
            continue

        if re.match(r"^\s*```\s*$", line):
            out.append("</code></pre>")
            in_code = False
            continue

        if in_table and re.match(r"^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)+\|?\s*$", line):
            continue

        if re.match(r"^\s*\|.*\|\s*$", line):
            if not in_table:
                out.append("<table>")
                in_table = True
            cells = re.split(r'\s*\|\s*', line.strip().strip('|'))
            cells = [escape_html(c) for c in cells]
            out.append("<tr>" + "".join(f"<td>{c}</td>" for c in cells) + "</tr>")
            continue
        else:
            if in_table:
                out.append("</table>")
                in_table = False

        if re.match(r"^####\s+", line):
            if in_list:
                out.append("</ul>")
                in_list = False
            text = re.sub(r"^####\s+", "", line)
            out.append(f"<h4>{process_inline_links(text)}</h4>")
            continue

        if re.match(r"^###\s+", line):
            if in_list:
                out.append("</ul>")
                in_list = False
            text = re.sub(r"^###\s+", "", line)
            out.append(f"<h3>{process_inline_links(text)}</h3>")
            continue

        if re.match(r"^##\s+", line):
            if in_list:
                out.append("</ul>")
                in_list = False
            text = re.sub(r"^##\s+", "", line)
            out.append(f"<h2>{process_inline_links(text)}</h2>")
            continue

        if re.match(r"^#\s+", line):
            if in_list:
                out.append("</ul>")
                in_list = False
            text = re.sub(r"^#\s+", "", line)
            out.append(f"<h1>{process_inline_links(text)}</h1>")
            continue

        if re.match(r"^\s*[-*]\s+", line):
            if not in_list:
                out.append("<ul>")
                in_list = True
            text = re.sub(r"^\s*[-*]\s+", "", line)
            out.append(f"<li>{process_inline_links(text)}</li>")
            continue

        if re.match(r"^\s*---+\s*$", line):
            if in_list:
                out.append("</ul>")
                in_list = False
            out.append("<hr>")
            out.append("")
            continue

        if re.match(r"^\s*$", line):
            if in_list:
                out.append("</ul>")
                in_list = False
            out.append("")
            continue

        if in_list:
            out.append("</ul>")
            in_list = False

        text = process_inline_links(line.strip())
        if text:
            out.append(f"<p>{text}</p>")

    if in_list:
        out.append("</ul>")
    if in_table:
        out.append("</table>")

    return "\n".join(out)


def build_page(input_file, output_file, title, nav_active=""):
    body = convert_md(input_file)

    nav_items = [
        ("index.html", "Home"),
        ("tutorial.html", "Tutorial"),
        ("cookbook.html", "Cookbook"),
        ("reference.html", "Reference"),
    ]
    nav_html = ""
    for href, label in nav_items:
        cls = ' class="active"' if href == nav_active else ""
        nav_html += f'<li><a href="{href}"{cls}>{label}</a></li>\n'

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title} — Nect</title>
<link rel="stylesheet" href="style.css">
</head>
<body>
<div class="sidebar">
  <div class="sidebar-header">
    <a href="index.html"><h1>Nect</h1></a>
    <span class="tagline">A small, fast scripting language</span>
  </div>
  <nav>
    <ul>
{nav_html}
    </ul>
  </nav>
  <div class="sidebar-footer">
    <a href="https://github.com">GitHub</a> ·
    <a href="https://crates.io/crates/nect">Crates.io</a>
  </div>
</div>
<div class="content">
{body}
</div>
</body>
</html>"""

    with open(output_file, "w", encoding="utf-8") as f:
        f.write(html)
    print(f"  → {output_file}")


def main():
    print("Building Nect documentation website...")

    build_page(
        os.path.join(DOCS_DIR, "tutorial.md"),
        os.path.join(SITE_DIR, "tutorial.html"),
        "Tutorial",
        "tutorial.html",
    )
    build_page(
        os.path.join(DOCS_DIR, "cookbook.md"),
        os.path.join(SITE_DIR, "cookbook.html"),
        "Cookbook",
        "cookbook.html",
    )
    build_page(
        os.path.join(DOCS_DIR, "reference.md"),
        os.path.join(SITE_DIR, "reference.html"),
        "Reference",
        "reference.html",
    )

    index_html = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Nect — A Small, Fast Scripting Language</title>
<link rel="stylesheet" href="style.css">
</head>
<body>
<div class="sidebar">
  <div class="sidebar-header">
    <a href="index.html"><h1>Nect</h1></a>
    <span class="tagline">A small, fast scripting language</span>
  </div>
  <nav>
    <ul>
      <li><a href="index.html" class="active">Home</a></li>
      <li><a href="tutorial.html">Tutorial</a></li>
      <li><a href="cookbook.html">Cookbook</a></li>
      <li><a href="reference.html">Reference</a></li>
    </ul>
  </nav>
  <div class="sidebar-footer">
    <a href="https://github.com">GitHub</a> ·
    <a href="https://crates.io/crates/nect">Crates.io</a>
  </div>
</div>
<div class="content">
<div class="hero">
<h1>Nect</h1>
<p class="lead">A small dynamically typed scripting language with three execution engines: a bytecode VM, a Cranelift JIT for hot numeric code, and a C backend that translates a program to a standalone native binary.</p>
</div>

<h2>Quick start</h2>
<pre><code class="language-bash">cargo build --release
./target/release/nect run examples/hello.nct
./target/release/nect run --interp examples/primes.nct
./target/release/nect check examples/arrays.nct
./target/release/nect disasm examples/arrays.nct</code></pre>

<h2>Learn the language</h2>
<ul>
<li><a href="tutorial.html">Tutorial</a> — the language taught from scratch, every sample's exact output.</li>
<li><a href="cookbook.html">Cookbook</a> — task-oriented recipes for text, arrays, loops, math, formatting.</li>
<li><a href="reference.html">Reference</a> — grammar, precedence, built-in reference, error catalogue, CLI, engines.</li>
</ul>

<h2>Examples</h2>
<p>Thirteen runnable programs in <code>examples/</code>, from <code>hello.nct</code> to <code>interpolation.nct</code>, <code>maps.nct</code>, <code>statistics.nct</code>, a complete <code>calculator.nct</code> with its own expression parser, and <code>webapp.nct</code>, a browser painting app.</p>

<h2>Compile to native</h2>
<pre><code class="language-bash">./target/release/nect build benches/heavy/dot_product.nct
./dot_product</code></pre>

<h2>Development</h2>
<pre><code class="language-bash">cargo test
cargo clippy --all-targets
bash benches/benchmark.sh</code></pre>

</div>
</body>
</html>"""
    with open(os.path.join(SITE_DIR, "index.html"), "w", encoding="utf-8") as f:
        f.write(index_html)
    print("  → index.html")

    print("Done! Open index.html in a browser.")


if __name__ == "__main__":
    main()
