# Recipes live in just/*.just, one file per concern; this root file keeps
# the aliases and the imports.

alias h := help
alias c := check
alias f := fmt
alias l := lint
alias t := test
alias p := prepare
alias pr := prepare

help:
    @just --list

import 'just/rust.just'
