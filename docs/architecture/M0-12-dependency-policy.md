# M0-12 – Dependency- und Feature-Policy

## Grundsätze

Der Workspace committet `Cargo.lock`; alle Verifikations- und Releasebuilds verwenden `--locked`. Produktdependencies stammen ausschließlich aus crates.io. Git-, fremde Registry- und externe lokale Path-Dependencies sind nicht freigegeben. Die Lizenzprüfung umfasst Runtime-, Test- und Build-Dependencies und erlaubt derzeit ausschließlich `MIT` und `Apache-2.0`, passend zur Projekt-Dual-Lizenz. Unbekannte Lizenzinformationen stoppen die Prüfung; es gibt keine aktiven Lizenz- oder Advisory-Ausnahmen.

`.cargo/deny.toml` pinnt die Prüfregeln für `cargo-deny 0.20.2`: Advisories, abgekündigte/unsound Dependencies, yanked Versions, Mehrfachversionen, Wildcard-Versionen, Sources und Lizenzen laufen fail-closed. Externe Default-Features sind standardmäßig verboten. Ein benötigtes Default-Feature muss mit crate-spezifischem Grund in der `cargo-deny`-Konfiguration und mit passender Prüfung in `policy/dependencies.tsv` dokumentiert werden. `anyhow`, `thiserror` und `failure` sind workspaceweit gesperrt.

## Inventar und Featurekombinationen

`policy/dependencies.tsv` enthält eine Zeile für jede aufgelöste externe Package-Version aus dem gelockten Graphen, auch für Transitives. Es hält Boundarynutzen, Maintainer-/Releaseprüfung, Lizenz und Advisoryprüfung, transitive Graphgröße, MSRV, Unsafe-, Build-/Proc-Macro-, Default-Feature- und Featureprüfung, Exit-Plan, Owner und Prüfdatum fest. `tools/check_dependency_policy.py` vergleicht Namen, Version und Source exakt mit `cargo metadata`, berechnet die Zahl erreichbarer transitiver Registry-Packages, prüft die erforderlichen Reviews und weist deklarierte MSRVs oberhalb des Projekt-MSRVs ab. Eine neue, geänderte oder entfernte Package-Version lässt den Verify-Lauf ohne passende Registerzeile fehlschlagen.

`policy/feature-matrix.tsv` ist die ausführbare Matrix für `--no-default-features`, Default-Features und `--all-features`. `tools/run_feature_matrix.py` validiert die drei Pflichtprofile und baut alle Workspace-Targets mit Lockfile. `cargo-deny` und der Metadatencheck lösen den Dependencygraphen mit allen Features auf. `cargo tree -e features` wird zusätzlich im Verify-Protokoll ausgegeben. Bewusst unzulässige featureübergreifende Paare stehen in `policy/feature-conflicts.tsv`; das Register prüft, dass beide Features existieren, und lässt den Verify-Lauf scheitern, sobald ein Paar im vollständigen Featuregraph gemeinsam aktiv ist. Das Register enthält aktuell keine Konflikte, weil der Workspace noch keine Produktfeatures definiert.

Für jeden künftigen Core-Fehlervertrag bleibt `Box<dyn Error>` im gesamten `worlddb-core/src` verboten. Der unabhängige Sourcecheck findet dieses Muster auch über Zeilenumbrüche hinweg. Zusätzlich sperrt `cargo-deny` die universellen Error-Crates im ganzen Workspace; Core-APIs verwenden benannte Domainfehler.

## Build- und Proc-Macro-Risiko

Der Dependency-Review weist Build-Dependencies, Build-Scripts und Proc-Macros ausdrücklich aus. `cargo-deny` verweigert native ausführbare Dateien und erkannte interpretierte Skripte in Workspace und Dependencies. Das ersetzt keine Quellcodeprüfung und keine isolierte Ausführung von Buildcode: Auch reine Rust-Build-Scripts und Proc-Macros brauchen in der Registerzeile eine dokumentierte manuelle Prüfung. `cargo-deny` selbst weist auf diese Erkennungsgrenze hin; die konkrete Prüfroutine ist in der Updatefolge unten festgelegt.

## Gepinntes Prüfwerkzeug

`policy/tool-versions.tsv` pinnt `cargo-deny 0.20.2`. Diese Version benötigt beim Bauen Rust 1.88.0. Dafür ist die separate Werkzeugtoolchain installiert; `rust-toolchain.toml`, Workspace-`rust-version` und MSRV-Tests bleiben auf Rust 1.85.0. Die Toolversion wird lokal vor dem eigentlichen Dependencycheck geprüft. Installation:

```powershell
rustup toolchain install 1.88.0 --profile minimal
cargo +1.88.0 install cargo-deny --version 0.20.2 --locked
```

Die Verify-Wrapper ergänzen `$CARGO_HOME/bin` beziehungsweise `%USERPROFILE%\.cargo\bin` zum `PATH`, damit der gepinnte Prüfer gefunden wird.

## Monatliche Dependency-Aktualisierung

Mindestens einmal pro Kalendermonat und zusätzlich bei einem relevanten Sicherheits- oder Lizenzhinweis wird ein kleiner Dependency-Update-Änderungssatz vorbereitet und geprüft:

1. Neue Dependencies erhalten vor der Aufnahme einen Eintrag mit Boundarynutzen, Registry, Lizenz, Maintainer-/Releasezustand, Advisorylage, Graphgröße, MSRV, Unsafe-Anteil, Featurebedarf, Build-/Proc-Macro-Risiko und Exit-Plan. Default-Features bleiben aus, außer die konkrete Notwendigkeit und die crate-spezifische Ausnahme sind begründet.
2. Die gelockten Versionen werden einzeln oder in einer eng zusammenhängenden Gruppe aktualisiert. `policy/dependencies.tsv` wird aus dem neuen tatsächlichen Graphen aktualisiert; Lizenzexpression, Registry, transitive Größe und Prüfdatum müssen dazu passen.
3. Der Änderungssatz läuft durch `cargo xtask verify` unter Rust 1.85.0. Der Advisorycheck aktualisiert die RustSec-Datenbank; MSRV-Builds, Default-/No-Default-/All-Features-Matrix, `cargo tree -e features`, Lizenz-, Source-, Ban- und Featurekonfliktchecks müssen bestehen.
4. Reviewer öffnen Build-Scripts und Proc-Macro-Quellcode der neu aufgenommenen oder geänderten Packages, suchen nach Unsafe und nativen/interpretierten Payloads und dokumentieren den Befund. Fehlende deklarierte MSRV wird durch den erfolgreichen gelockten MSRV-Build belegt.
5. Update, Lockfile und Reviewregister werden gemeinsam lokal versioniert. Bis zur separat zurückgestellten GitHub-Verknüpfung wird die Änderung lokal in einem prüfbaren Branch begutachtet; ein Remote-PR ist keine Voraussetzung für diesen lokalen Planstand.

Die periodische Review belegt keine SBOM- oder Releasefreigabe. Release-SBOM und Advisory-Snapshot bleiben bei M9-09.

## Referenzen

- [cargo-deny 0.20.2 Paket- und MSRV-Metadaten](https://crates.io/crates/cargo-deny/0.20.2)
- [cargo-deny Bans- und Featurekonfiguration](https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html)
- [cargo-deny Advisorykonfiguration](https://embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html)
- [cargo-deny Lizenzkonfiguration](https://embarkstudios.github.io/cargo-deny/checks/licenses/cfg.html)
- [cargo-deny Sourcekonfiguration](https://embarkstudios.github.io/cargo-deny/checks/sources/cfg.html)
