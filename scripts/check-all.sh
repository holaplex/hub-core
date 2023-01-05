#!/bin/bash

set -e
cd "$(dirname "$0")/.."

[[ -z "$CARGO" ]] && CARGO=cargo

"$CARGO" fmt --all --check

json="$(cargo metadata --format-version 1 --no-deps)"

lint_pats=(
'#! [ deny (
  clippy :: disallowed_methods ,
  clippy :: suspicious ,
  clippy :: style ,
  missing_debug_implementations ,
  missing_copy_implementations ,
) ]'
'#! [ warn (
  clippy :: pedantic ,
  clippy :: cargo ,
  missing_docs ,
) ]'
)
lint_res=()

for pat in "${lint_pats[@]}"; do
  lint_res+=("$( \
    echo -n "$pat" | \
    tr '\n' ' ' | \
    sed -r 's/, +\)/,? )/g;s/[()]/\\\0/g;s/ +/[\\s\\x00]*/g'
  )")
done

for pkg in $(jq -r '.packages | keys | join("\n")' <<<"$json"); do
  pj="$(jq ".packages[$pkg]" <<<"$json")"

  manifest="$(jq -r '.manifest_path' <<<"$pj")"

  for tgt in $(jq -r '.targets | keys | join("\n")' <<<"$pj"); do
    tj="$(jq ".targets[$tgt]" <<<"$pj")"

    name="$(jq -r '.name' <<<"$tj")"
    kinds="$(jq -r '.kind | join("/")' <<<"$tj")"
    types="$(jq -r '.crate_types | join("/")' <<<"$tj")"
    src="$(jq -r '.src_path' <<<"$tj")"

    # Not sure why lib is like that but I don't make the rules
    case "$kinds" in
      bin) kind=bin ;;
      custom-build) continue ;;
      "$types") kind=lib ;;
      *) kind="$kinds"
    esac

    [[ -t 1 ]] && echo -n $'\x1b[1m'
    echo "==> $name ($kind)"
    [[ -t 1 ]] && echo -n $'\x1b[m'

    # Check lints
    i=0
    for re in "${lint_res[@]}"; do
      if ! grep -Pzq "$re" "$src"; then
        echo "File '$src' missing required lints!"
        echo $'Consider adding:\n'"${lint_pats[$i]}"
        false
      fi
      i="$(( i + 1 ))"
    done

    case "$kind" in
      */*|*lib|proc-macro) flags=(--lib) ;;
      *) flags=(--"$kind" "$name") ;;
    esac

    flags+=(--manifest-path "$manifest" --all-features)

    "$CARGO" clippy "${flags[@]}" --no-deps
    "$CARGO" doc "${flags[@]}" --no-deps
    "$CARGO" build "${flags[@]}"
    [[ "$kind" == test ]] || "$CARGO" test "${flags[@]}"
  done
done
