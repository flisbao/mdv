#!/usr/bin/env bash
# Atualiza os blocos gerados do README a cada release.
# Uso: scripts/update-readme.sh <tag> <caminho-do-binario-mdv> [README.md]
#
# Blocos (entre <!-- mdv:NOME:start --> e <!-- mdv:NOME:end -->):
#   install  links de download da versão <tag>
#   help     saída real de `mdv --help` do binário publicado
set -euo pipefail

tag=${1:?uso: update-readme.sh <tag> <binario> [README.md]}
bin=${2:?uso: update-readme.sh <tag> <binario> [README.md]}
readme=${3:-README.md}
repo=${GITHUB_REPOSITORY:-flisbao/mdv}
base="https://github.com/$repo/releases/download/$tag"

install=$(cat <<BLOCK
| Plataforma | Download |
|------------|----------|
| Linux x86_64 (glibc 2.35+) | [\`mdv-$tag-linux-x86_64.tar.gz\`]($base/mdv-$tag-linux-x86_64.tar.gz) |
| Linux x86_64 (estático, qualquer distro, Alpine inclusive) | [\`mdv-$tag-linux-x86_64-musl.tar.gz\`]($base/mdv-$tag-linux-x86_64-musl.tar.gz) |
| Linux aarch64 | [\`mdv-$tag-linux-aarch64.tar.gz\`]($base/mdv-$tag-linux-aarch64.tar.gz) |
| macOS Apple Silicon | [\`mdv-$tag-macos-aarch64.tar.gz\`]($base/mdv-$tag-macos-aarch64.tar.gz) |
| macOS Intel | [\`mdv-$tag-macos-x86_64.tar.gz\`]($base/mdv-$tag-macos-x86_64.tar.gz) |
| Windows x86_64 | [\`mdv-$tag-windows-x86_64.zip\`]($base/mdv-$tag-windows-x86_64.zip) |
| Debian/Ubuntu e Fedora/RHEL | pacotes \`.deb\` e \`.rpm\` na [página da release](https://github.com/$repo/releases/tag/$tag) |

Versão atual: **$tag**. Cada release traz um \`SHA256SUMS\` para conferir os arquivos.
BLOCK
)

help=$(printf '```text\n%s\n```' "$("$bin" --help)")

replace() { # nome conteúdo
  local name=$1 content=$2 start end
  start="<!-- mdv:$name:start -->"
  end="<!-- mdv:$name:end -->"
  grep -qF "$start" "$readme" || { echo "marcador $start ausente em $readme" >&2; exit 1; }
  CONTENT=$content awk -v s="$start" -v e="$end" '
    $0 == s { print; print ENVIRON["CONTENT"]; skip = 1; next }
    $0 == e { skip = 0 }
    !skip   { print }
  ' "$readme" > "$readme.tmp"
  mv "$readme.tmp" "$readme"
}

replace install "$install"
replace help "$help"
