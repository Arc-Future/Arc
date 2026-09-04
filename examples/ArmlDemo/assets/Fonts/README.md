# Demo fonts (RFC 037 §9)

Place `.ttf` / `.otf` files here (e.g. `AppSans.ttf`).

`arc build` copies this tree to `bin/<Config>/Assets/` so
`Application.Fonts.RegisterFamily("AppSans", "Assets/Fonts/AppSans.ttf")`
resolves against the application base directory (exe dir).

Do not commit large vendor font binaries; keep only small samples or local copies.
