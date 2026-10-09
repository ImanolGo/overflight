_overflight() {
    local i cur prev opts cmd
    COMPREPLY=()
    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
        cur="$2"
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
    fi
    prev="$3"
    cmd=""
    opts=""

    for i in "${COMP_WORDS[@]:0:COMP_CWORD}"
    do
        case "${cmd},${i}" in
            ",$1")
                cmd="overflight"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        overflight)
            opts="-h -V --lat --lon --alt --source --url --demo --radius-km --min-elevation --interval --screensaver --log --no-mouse --no-routes --bell --colors --dump --record --time --tle-group --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --lat)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --lon)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --alt)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --source)
                    COMPREPLY=($(compgen -W "adsb-lol airplanes-live local opensky" -- "${cur}"))
                    return 0
                    ;;
                --url)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --radius-km)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --min-elevation)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --interval)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --log)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --colors)
                    COMPREPLY=($(compgen -W "auto truecolor 256 none" -- "${cur}"))
                    return 0
                    ;;
                --record)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --time)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --tle-group)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _overflight -o nosort -o bashdefault -o default overflight
else
    complete -F _overflight -o bashdefault -o default overflight
fi
