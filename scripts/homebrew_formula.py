# SPDX-License-Identifier: MIT OR Apache-2.0
"""Print the Homebrew formula (Formula/chronostack.rb in Asmodeus14/homebrew-tap)
for a published release, using the checksums from its SHA256SUMS file.

Usage: python scripts/homebrew_formula.py 0.1.0-alpha.7 path/to/SHA256SUMS
"""
import sys

REPO = "Asmodeus14/ChronoStack"
# (Homebrew block, release archive suffix)
TARGETS = {
    "macos": [("on_arm", "darwin-arm64"), ("on_intel", "darwin-x64")],
    "linux": [("on_arm", "linux-arm64"), ("on_intel", "linux-x64")],
}


def checksums(text):
    sums = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) == 2:
            sums[parts[1].lstrip("*")] = parts[0].lower()
    return sums


def formula(version, sums):
    def block(os_name):
        out = [f"  on_{os_name} do"]
        for arch_block, suffix in TARGETS[os_name]:
            name = f"chronostack-{version}-{suffix}.tar.gz"
            if name not in sums:
                sys.exit(f"{name} is not listed in SHA256SUMS")
            out += [
                f"    {arch_block} do",
                f'      url "https://github.com/{REPO}/releases/download/v{version}/{name}"',
                f'      sha256 "{sums[name]}"',
                "    end",
            ]
        out.append("  end")
        return "\n".join(out)

    return f'''class Chronostack < Formula
  desc "Stripe Test Clocks for your Docker Compose stack"
  homepage "https://github.com/{REPO}"
  license any_of: ["MIT", "Apache-2.0"]

{block("macos")}

{block("linux")}

  def install
    bin.install "chronostack"
  end

  def caveats
    <<~EOS
      ChronoStack needs Docker with Compose v2 (Docker Desktop, OrbStack or Colima).
      Upgrade with `brew upgrade chronostack` rather than `chronostack update`.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{{bin}}/chronostack --version")
  end
end
'''


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    version = sys.argv[1].lstrip("v")
    with open(sys.argv[2], encoding="utf-8") as f:
        # bytes, so Windows doesn't turn the newlines into CRLF
        sys.stdout.buffer.write(formula(version, checksums(f.read())).encode("utf-8"))
