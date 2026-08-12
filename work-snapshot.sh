#!/bin/bash

SOURCE_DIR=/path/to/your/work
STORAGE_DIR=/path/to/backups/work.snapshots

self_name="$(basename $0)";

build-cargo-targets-exclude() {
    local package_files="$(find $SOURCE_DIR -type f -name "Cargo.toml")"
    for package_file in $package_files; do
        local project_dir=$(dirname "$package_file")
        if [[ -d "$project_dir/target" ]]; then
            echo "$project_dir/target" | sed "s#^${SOURCE_DIR%/}/#--exclude ./#g"
        fi
    done
    local anchor_expanded_targets="$(find $SOURCE_DIR -type d -wholename "*/.anchor/expanded-macros/expand-target")"
    for expanded_target in $anchor_expanded_targets; do
        echo "$expanded_target" | sed "s#^${SOURCE_DIR%/}/#--exclude ./#g"
    done
}

build-php-vendor-exclude() {
    local package_files="$(find $SOURCE_DIR -type f -name "composer.json")"
    for package_file in $package_files; do
        local project_dir=$(dirname "$package_file")
        if [[ -d "$project_dir/vendor" ]]; then
            echo "$project_dir/vendor" | sed "s#^${SOURCE_DIR%/}/#--exclude ./#g"
        fi
    done
}

build-snapshot-ignore-excludes() {
    local ignore_files="$(find $SOURCE_DIR -type f -name ".worksnap-ignore-dir")"
    for ignore_file in $ignore_files; do
        local ignore_dir="$(dirname "$ignore_file")"
        if [[ -d "$ignore_dir" ]]; then
            echo "$ignore_dir" | sed "s#^${SOURCE_DIR%/}/#--exclude ./#g"
        fi
    done
}


find-last-base() {
    local list="$(ls $STORAGE_DIR/*.snar 2>/dev/null)"
    if [ "$list" = "" ]; then return 1; fi
    local last_base="$(echo "$list" | sort -r | head -n1 | xargs basename | sed -E 's/([0-9]{4}(-[0-9]{2}){2}-[0-9]{4}).*\.snar/\1/g')"
    echo "$last_base"
}

find-last-full-base() {
    local list="$(find $STORAGE_DIR -maxdepth 1 -type f  -name "*.tar.gz" -and \! -name "*.based-on-*.tar.gz")"
    local last_base="$(echo "$list" | sort -r | head -n1 | xargs basename | sed -E 's/([0-9]{4}(-[0-9]{2}){2}-[0-9]{4})\.tar.gz/\1/g')"
    if [ ! -f "$STORAGE_DIR/$last_base.snar" ]; then
        echo "Panic: full archive \"$STORAGE_DIR/$last_base.tar.gz\" doesn't have appropriate snar-file" >&2
        exit 11
    fi
    echo "$last_base"
}

find-snar-by-timestamp() {
    if [ "$1" = "" ]; then
        echo "Error: find-snar-by-timestamp: argument is empty" >&2;
        return 101
    fi
    local list
    list="$(ls $STORAGE_DIR/$1*.snar 2>/dev/null)"
    if [ "$list" = "" ]; then
        echo "Error: snar file has not been found: timestamp=\"$1\"" >&2
        return 5
    fi
    echo "$list" | sort -r | head -n1 | xargs basename
}

COMMON_EXCLUDES="--exclude node_modules"
COMMON_EXCLUDES="${COMMON_EXCLUDES} --exclude .venv"
COMMON_EXCLUDES="${COMMON_EXCLUDES} --exclude .pnpm-store"
COMMON_EXCLUDES="${COMMON_EXCLUDES} --exclude .Trash-${UID}"
COMMON_EXCLUDES="${COMMON_EXCLUDES} --exclude test-ledger"
COMMON_EXCLUDES="${COMMON_EXCLUDES} --exclude ./go/pkg"
COMMON_EXCLUDES="${COMMON_EXCLUDES} --exclude ./.cache/uv"
COMMON_EXCLUDES="${COMMON_EXCLUDES} --exclude dhat.out.*"
COMMON_EXCLUDES="${COMMON_EXCLUDES} --exclude heaptrack.*"

create-full() {
    local timestamp
    if [ "$1" = "" ]; then timestamp="$(date +%Y-%m-%d-%H%M)"; else timestamp="$1"; fi
    local cargo_tragets_exclude="$(build-cargo-targets-exclude)"
    local php_vendor_exclude="$(build-php-vendor-exclude)"
    local ignore_excludes="$(build-snapshot-ignore-excludes)"
    tar -C $SOURCE_DIR \
        --listed-incremental="$STORAGE_DIR/$timestamp.snar" \
        -czvpf "$STORAGE_DIR/$timestamp.tar.gz" \
        --one-file-system \
        --sparse \
        $cargo_tragets_exclude \
        $php_vendor_exclude \
        $ignore_excludes \
        $COMMON_EXCLUDES \
        .
    local tar_ret=$?
    ls -lh "$STORAGE_DIR/$timestamp.tar.gz" >&2
    return $tar_ret
}

create-incremental() {
    local base="$1"
    local timestamp="$(date +%Y-%m-%d-%H%M)"
    local base_snar="$(find-snar-by-timestamp $base)"
    if [ "$base_snar" = "" ]; then return 12; fi
    local cargo_tragets_exclude="$(build-cargo-targets-exclude)"
    local php_vendor_exclude="$(build-php-vendor-exclude)"
    local ignore_excludes="$(build-snapshot-ignore-excludes)"
    cp "$STORAGE_DIR/$base_snar" "$STORAGE_DIR/$timestamp.based-on-$base.snar" \
        && tar -C $SOURCE_DIR \
            --listed-incremental="$STORAGE_DIR/$timestamp.based-on-$base.snar" \
            -czpf "$STORAGE_DIR/$timestamp.based-on-$base.tar.gz" \
            --one-file-system \
            --sparse \
            $cargo_tragets_exclude \
            $php_vendor_exclude \
            $ignore_excludes \
            $COMMON_EXCLUDES \
            .
    local tar_ret=$?
    ls -lh "$STORAGE_DIR/$timestamp.based-on-$base.tar.gz" >&2
    return $tar_ret
}

check-timestamp-format() {
    if [ "" = "$(echo "$1" | grep -E '^[0-9]{4}-[0-9]{2}-[0-9]{2}-[0-9]{4}$')" ]; then
        echo "Error: wrong base timestamp format" >&2
        return 4
    fi
}

show-help() {
    echo "Usage: ${self_name} [options]"
    echo "  -F | --full             - create new full archive"
    echo "  -h | --help             - show this help message"
    echo "  -b | --base=<timestamp> - set timestamp of the archive for incremental backup"
    echo "                            <timestamp> may also be \"last\" or \"last-full\""
    echo "                            \"full\" is an alias for \"last-full\""
    echo "  -S | --find=<last|full> - find last or full base"
    echo "       --from             - alias for --base"
    echo "  -l | --base=last        - alias for --base=last"
    echo "  -f | --base=full        - alias for --base=full or --base=last-full"
}

main() {
    local opts=$(getopt -o "b:S:lfFh" --long 'base:,from:,find:,full,help' -n 'parse-options' -- $@)
    eval set -- $opts;

    local base=""
    local archive_type="incremental"

    while (( $# )); do
        #echo "Opts: $@ | count:$#";
        case $1 in
            -b|--base|--from)
                if [ "$2" = "" ]; then
                    echo "Error: base timestamp is undefined" >&2
                    return 3
                fi
                base="$2"
                shift; shift
            ;;
            -S|--find)
                if [ "$2" = "last" ]; then
                    find-last-base
                elif [ "$2" = "full" ]; then
                    find-last-full-base
                else
                    echo "Error: Invalid archive type (available 'last' or 'full')"
                    return 4
                fi
                return 0
            ;;
            -l) base="last"; shift ;;
            -f) base="last-full"; shift ;;
            -F|--full) archive_type="full"; shift ;;
            -h|--help) show-help; return 0 ;;
            *) if [ "$2" != "" ]; then show-help; fi; shift ;;
        esac
    done

    case "$archive_type" in
        incremental)
            if [ "$base" = "" ] || [ "$base" = "last" ]; then
                base="$(find-last-base || echo "")";
            elif [ "$base" = "last-full" ] || [ "$base" = "full" ]; then
                base="$(find-last-full-base || echo "")";
            fi
            if [ "$base" = "" ]; then "Error: base archive is undefined" >&2 ; return 1; fi
            check-timestamp-format "$base" || return $?
            find-snar-by-timestamp $base > /dev/null || return $?
            create-incremental $base || return $?
        ;;
        full)
            if [ "$base" != "" ]; then
                echo "Warning: --base option is not necessary for the full archive creation" >&2
            fi
            create-full || return $?
        ;;
        *)
            echo "Error: not_reachable: undefined archive type (new base or incremental?)" >&2
            return 2
        ;;
    esac
}

main $@
#find-last-base
#find-last-full-base
#find-snar-by-timestamp $@
exit $?
