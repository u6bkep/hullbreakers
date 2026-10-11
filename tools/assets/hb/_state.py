"""Process-wide state shared between the runner (tools/assets/run_build.py) and hb."""
current = None       # the hb.Asset being built
preview = False      # render previews after export (set by the runner)
exported = None      # path of the .glb once export() has run
