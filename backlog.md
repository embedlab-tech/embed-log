# Backlog `embed-log`

Cel: uporządkowany plan prac do realizacji krok po kroku.
Status wszystkich pozycji: `TODO`.

---

## Plan realizacji (proponowana kolejność)

### Etap 1 — Krytyczne bugi i spójność eksportu (start tutaj)
- **BL-05** Zdublowane timestampy w `session.html`
- **BL-09** Niedziałający dark/light mode w `session.html`
- **BL-11** Uproszczenie workflow `session.html` (zapis, statusy, eventy)
- **BL-06** Timezone (lokalna strefa + przenośność)

### Etap 2 — Stabilność platformy i UX core
- **BL-01** Kompatybilność Python 3.10
- **BL-03** Widoczna nazwa przy jednej zakładce
- **BL-04** Usunięcie dynamicznego tworzenia zakładek
- **BL-07** Nazwa pliku eksportu z configu (cache przy starcie)

### Etap 3 — Usprawnienia UX (nice-to-have)
- **BL-02** Unwrap zakładek side-by-side
- **BL-08** Scrollbar dark mode
- **BL-10** Compact UI

---

## Zadania szczegółowe

## BL-01 — Kompatybilność z Python 3.10
- **Typ:** Usprawnienie / kompatybilność
- **Priorytet:** P1
- **Obszar:** Backend, CI, packaging, docs

### Taski
- [ ] Zmienić `requires-python` w `pyproject.toml` na `>=3.10`.
- [ ] Zweryfikować zależności (`pyserial`, `aiohttp`, `PyYAML`) dla 3.10.
- [ ] Uruchomić testy i podstawowe flow (`validate`, `run`) na 3.10.
- [ ] Dodać matrix CI: 3.10/3.11/3.12.
- [ ] Zaktualizować `README.md` i `INSTALL.md`.

### DoD
- Testy przechodzą na 3.10.
- Demo/run działa bez regresji.

---

## BL-02 — Unwrap zakładek po merge side-by-side
- **Typ:** Usprawnienie UX
- **Priorytet:** P2
- **Obszar:** Frontend (tabs/layout)

### Taski
- [ ] Dodać akcję UI „Unwrap”.
- [ ] Rozdzielić split na 2 osobne taby.
- [ ] Zachować kolejność, tytuły, filtry i źródła.
- [ ] Sprawdzić persistence/cache layoutu.

### DoD
- Jednym działaniem można rozdzielić merged tab.
- Brak regresji merge/swap/layout.

---

## BL-03 — Nazwa zakładki widoczna także przy 1 zakładce
- **Typ:** Bug / UX
- **Priorytet:** P2
- **Obszar:** Frontend (tabs rendering)

### Taski
- [ ] Ujednolicić render tab bara dla 1 i wielu zakładek.
- [ ] Zweryfikować przejścia 1↔2+.
- [ ] Sprawdzić odświeżenie i restore z cache.

### DoD
- Nazwa pojedynczej zakładki zawsze widoczna.

---

## BL-04 — Usunąć dynamiczne tworzenie zakładek
- **Typ:** Zmiana funkcjonalna / uproszczenie
- **Priorytet:** P2
- **Obszar:** Frontend (tabcreate/tabs/UI)

### Taski
- [ ] Usunąć UI/eventy dodawania zakładek runtime.
- [ ] Wyłączyć logikę dynamicznego tworzenia tabów.
- [ ] Zostawić poprawne działanie zakładek z configu.
- [ ] Dostosować persistence/cache.
- [ ] Zaktualizować docs (jeśli potrzebne).

### DoD
- Brak opcji dynamicznego dodawania zakładek.
- Działają taby konfiguracyjne bez regresji.

---

## BL-05 — Zdublowane systemowe timestampy w `session.html`
- **Typ:** Bug
- **Priorytet:** P1
- **Obszar:** Backend + Frontend

### Taski
- [ ] Odtworzyć błąd i wskazać źródło (backend czy render).
- [ ] Naprawić duplikację timestampu.
- [ ] Dodać test regresyjny eksportu HTML.
- [ ] Sprawdzić brak wpływu na live view.

### DoD
- W `session.html` jest tylko 1 systemowy timestamp na linię.

---

## BL-06 — Obsługa systemowej strefy czasowej (nie tylko UTC)
- **Typ:** Usprawnienie / poprawność czasu
- **Priorytet:** P1
- **Obszar:** Backend (czas/serializacja) + Frontend (prezentacja)

### Taski
- [ ] Ustalić model czasu: canonical UTC + prezentacja lokalna (lub tryb konfigurowalny).
- [ ] Przekazywać offset/strefę (np. ISO 8601 z offsetem).
- [ ] Zapewnić przenośność Linux/macOS/Windows.
- [ ] Sprawdzić live view + `session.html` + API sesji.
- [ ] Dodać testy dla min. 2 stref czasowych.

### DoD
- UI i eksport pokazują czas zgodnie z ustalonym modelem.
- Zachowanie spójne między systemami.

---

## BL-07 — Nazwa pliku eksportu HTML zgodna z configiem
- **Typ:** Usprawnienie UX
- **Priorytet:** P2
- **Obszar:** Frontend export + backend config payload

### Taski
- [ ] Ustalić schemat nazwy (np. `app_name`, `session_id`, czas).
- [ ] Przekazać dane nazewnicze w `config` na starcie.
- [ ] Scache’ować te wartości po stronie frontendu.
- [ ] Dodać fallbacki dla brakujących pól.
- [ ] Ujednolicić nazewnictwo manual/auto export.

### DoD
- Nazwy eksportu są spójne, przewidywalne i bezpieczne.

---

## BL-08 — Scrollbar dark mode (kolor i kontrast)
- **Typ:** Bug / UX (styling)
- **Priorytet:** P3
- **Obszar:** Frontend CSS/theme

### Taski
- [ ] Ustawić dedykowany styl scrollbar dla dark mode.
- [ ] Dodać wsparcie WebKit + Firefox (+ fallbacki).
- [ ] Sprawdzić brak regresji w light mode.

### DoD
- Scrollbar w dark mode jest czytelny i nierażący.

---

## BL-09 — W `session.html` nie działa przełączanie dark/light
- **Typ:** Bug
- **Priorytet:** P1
- **Obszar:** Frontend (offline viewer `session.html`)

### Taski
- [ ] Odtworzyć błąd (motyw „zamrożony” przy eksporcie).
- [ ] Naprawić inicjalizację i toggle motywu dla pliku offline.
- [ ] Ustalić priorytet: zapis użytkownika vs `prefers-color-scheme` vs fallback.
- [ ] Dodać checklistę/test regresyjny.

### DoD
- Po eksporcie `session.html` można przełączać dark/light.

---

## BL-10 — Compact UI (więcej miejsca na logi)
- **Typ:** Usprawnienie UX (nice-to-have)
- **Priorytet:** P3
- **Obszar:** Frontend layout/styling

### Taski
- [ ] Zmniejszyć wysokości/paddingi kluczowych elementów UI.
- [ ] Dodać przełącznik Compact UI.
- [ ] Zapisać preferencję użytkownika.
- [ ] Sprawdzić wpływ na splittery/tabs/popupy.

### DoD
- Tryb compact daje ~1–2 linie więcej na logi bez regresji UX.

---

## BL-11 — Uproszczony workflow `session.html` (zapis, dostęp, statusy)
- **Typ:** Usprawnienie UX + funkcjonalność
- **Priorytet:** P1
- **Obszar:** Backend session/export/events/API + Frontend toolbar/sessions

### Taski
- [ ] Dodać przycisk „Zapisz sesję do HTML” (jasny feedback sukces/błąd).
- [ ] Wysyłać na WS connect stan HTML (`exists/url/updated_at`).
- [ ] Uprościć statusy (np. `ready`, `updating`, `error`).
- [ ] Dodać eventy WS dla utworzenia/aktualizacji HTML.
- [ ] Uprościć otwieranie HTML i artefaktów plikowych z UI.
- [ ] Zaktualizować API kontrakty i dokumentację.

### DoD
- Użytkownik ma prosty, czytelny flow zapisu i otwierania `session.html`.
- Statusy i eventy są jednoznaczne i przewidywalne.

---

## BL-12 — Domyślne ustawienia: zapis i konfiguracja przez YAML
- **Typ:** Usprawnienie funkcjonalne
- **Priorytet:** P2
- **Obszar:** Backend config + Frontend settings/persistence

### Taski
- [ ] Zdefiniować listę ustawień, które mogą mieć wartości domyślne (np. theme, layout/UI mode, filtry, zachowanie eksportu).
- [ ] Umożliwić ustawienie tych defaultów w `embed-log.yml`.
- [ ] Ustalić priorytet źródeł konfiguracji: CLI > YAML > zapisane preferencje użytkownika > fallback systemowy.
- [ ] Dodać mechanizm zapisu bieżących ustawień jako nowych defaultów (opcjonalnie: akcja „Save as defaults”).
- [ ] Zapewnić zgodność z istniejącym cache sesji i brak konfliktów między sesją a global default.
- [ ] Zaktualizować dokumentację i przykładowy config.

### DoD
- Użytkownik może zdefiniować domyślne ustawienia w YAML.
- (Jeśli wdrożone) Użytkownik może zapisać aktualne ustawienia jako defaulty.
- Reguły nadpisywania ustawień są jasne i działają przewidywalnie.
