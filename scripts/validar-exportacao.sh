#!/usr/bin/env bash
# Valida a exportação com o LibreOffice: converte de volta cada arquivo gerado e confere o conteúdo.
# Uso: scripts/validar-exportacao.sh   (precisa de `soffice` e de `cargo build --release` feito)
set -euo pipefail
MDV="${MDV:-target/release/mdv}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
command -v soffice >/dev/null || { echo "soffice não encontrado" >&2; exit 2; }
# Perfil temporário e prazo: não depende do perfil do usuário nem trava.
lo() { timeout 120 soffice -env:UserInstallation="file://$TMP/lo-profile" --headless "$@" >/dev/null 2>&1; }

# 1) Planilhas: exporta → LibreOffice converte para CSV → compara com o esperado.
"$MDV" samples/tabelas-export.md -o "$TMP/t.xlsx" --section Vendas
"$MDV" samples/tabelas-export.md -o "$TMP/t.ods" --section Vendas
for f in t.xlsx t.ods; do
  lo --convert-to csv --outdir "$TMP/csv-$f" "$TMP/$f"
  csv="$TMP/csv-$f/${f%.*}.csv"
  grep -q '^produto,região,quantidade,preço' "$csv" || { echo "FALHA: cabeçalho de $f"; exit 1; }
  grep -q 'maçã,Sul,120,3.5' "$csv" || { echo "FALHA: linha de $f"; exit 1; }
  grep -q '007' "$csv" || { echo "FALHA: zero à esquerda perdido em $f"; exit 1; }
  echo "ok: $f"
done

# 2) Fórmula nunca é executada: a célula '=SOMA(A1:A2)' deve sair literal.
"$MDV" samples/tabelas-export.md -o "$TMP/c.xlsx" --section Custos
lo --convert-to csv --outdir "$TMP/csv-c" "$TMP/c.xlsx"
grep -q '=SOMA(A1:A2)' "$TMP/csv-c/c.csv" || { echo "FALHA: fórmula foi avaliada"; exit 1; }
echo "ok: fórmula literal"

# 3) CSV → XLSX/ODS (acentos e aspas).
printf 'nome,nota\n"Ana ""A""",9.5\nção,007\n' > "$TMP/d.csv"
for ext in xlsx ods; do
  "$MDV" "$TMP/d.csv" -o "$TMP/d.$ext"
  lo --convert-to csv --outdir "$TMP/csv-d-$ext" "$TMP/d.$ext"
  diff <(cat "$TMP/csv-d-$ext/d.csv") <(printf 'nome,nota\n"Ana ""A""",9.5\nção,007\n') || { echo "FALHA: roundtrip $ext"; exit 1; }
  echo "ok: csv → $ext"
done

# 4) Documentos: LibreOffice abre DOCX/ODT e converte para PDF; o PDF nativo é conferido por pdftotext, se houver.
for ext in docx odt; do
  "$MDV" samples/exportar.md -o "$TMP/doc.$ext"
  lo --convert-to pdf --outdir "$TMP/pdf-$ext" "$TMP/doc.$ext"
  [ -s "$TMP/pdf-$ext/doc.pdf" ] || { echo "FALHA: $ext não abriu"; exit 1; }
  echo "ok: $ext abre"
done
"$MDV" samples/exportar.md -o "$TMP/doc.pdf"
if command -v pdftotext >/dev/null; then
  pdftotext "$TMP/doc.pdf" - | grep -q 'Amostra de exportação' || { echo "FALHA: texto do PDF"; exit 1; }
fi
echo "ok: pdf"
echo "TUDO OK"
