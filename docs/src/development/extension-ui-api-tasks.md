# Extension UI API — Danh sách task

Tài liệu thiết kế: [extension-ui-api.md](./extension-ui-api.md)

**Trạng thái:** ⬜ Chưa bắt đầu · 🟦 Đang làm · 🟨 Đang review · ✅ Xong · ⛔ Bị chặn

Cập nhật lần cuối: 2026-10-04

**Phân công:** Lead = phần nền dùng chung (type UI, trait `Extension`, manifest, proxy) · Agent A = workspace + crate `extension_panel` · Agent B = WIT + guest API + host binding · Agent C = vòng đời extension + extension mẫu + test

> P3-1 (manifest) và P5-3 (validate) đã được làm trong phần nền: `crates/extension/src/types/ui.rs`, `extension_manifest.rs`, `extension_host_proxy.rs`.

## Tổng quan

| Phase | Nội dung | Số task | Xong | Trạng thái |
|---|---|---|---|---|
| 0 | Chuẩn bị | 2 | 1 | 🟦 |
| 4a | Spike `Panel` trait | 5 | 5 | ✅ |
| 1 | WIT 0.9.0 + guest API | 5 | 5 | ✅ |
| 2 | Host binding | 4 | 4 | ✅ |
| 3 | Manifest + proxy + registry | 6 | 6 | ✅ |
| 4b | Renderer | 4 | 4 | ✅ |
| 5 | Event loop + ổn định | 5 | 5 | ✅ |
| 6 | Extension mẫu + test | 8 | 8 | ✅ |
| 8 | Widget mở rộng + Git API + extension Source Control | 9 | 9 | 🟨 |
| **Tổng** | | **48** | **47** | |

## Phase 0 — Chuẩn bị

| ID | Task | Phụ thuộc | Ước lượng | Người làm | Trạng thái |
|---|---|---|---|---|---|
| P0-1 | Tạo branch `extension-ui-api` | — | 0.1d | Lead | ✅ |
| P0-2 | Review tài liệu thiết kế với team, chốt danh sách widget MVP | — | 0.4d | | ⬜ |

## Phase 4a — Spike `Panel` trait (làm trước, rủi ro cao nhất)

| ID | Task | Phụ thuộc | Ước lượng | Người làm | Trạng thái |
|---|---|---|---|---|---|
| P4a-1 | Thêm method `&self` cho `persistent_name` / `panel_key` / `icon_tooltip` vào `Panel` (có default impl), cập nhật `PanelHandle` trong `crates/workspace/src/dock.rs` | P0-1 | 1d | Agent A | ✅ |
| P4a-2 | Tạo crate `extension_panel` (`[lib] path = "src/extension_panel.rs"`) | P0-1 | 0.5d | Agent A | ✅ |
| P4a-3 | Action `ToggleExtensionPanel { panel_id }` + helper tìm panel theo `panel_id` | P4a-1 | 0.5d | Agent A | ✅ |
| P4a-4 | `ExtensionPanel` render một `UiTree` hardcode | P4a-2 | 1d | Agent A | ✅ |
| P4a-5 | Kiểm tra 2 panel cùng type: toggle, persist vị trí/kích thước độc lập | P4a-3, P4a-4 | 0.5d | Agent A | ✅ |

## Phase 1 — WIT 0.9.0 + guest API

| ID | Task | Phụ thuộc | Ước lượng | Người làm | Trạng thái |
|---|---|---|---|---|---|
| P1-1 | Copy `wit/since_v0.8.0` → `wit/since_v0.9.0` | P0-1 | 0.2d | Agent B | ✅ |
| P1-2 | Viết `ui.wit` (node, ui-tree, ui-event, `request-render`) | P1-1 | 0.5d | Agent B | ✅ |
| P1-3 | Thêm `panel-render`, `panel-handle-event` vào `extension.wit` | P1-2 | 0.3d | Agent B | ✅ |
| P1-4 | Guest: trait methods có default impl, `request_render`, bump `zed_extension_api` lên 0.9.0 | P1-3 | 0.5d | Agent B | ✅ |
| P1-5 | Guest: builder `ui::*` flatten cây lồng nhau thành arena | P1-4 | 1d | Agent B | ✅ |

## Phase 2 — Host binding

| ID | Task | Phụ thuộc | Ước lượng | Người làm | Trạng thái |
|---|---|---|---|---|---|
| P2-1 | Tạo `wasm_host/wit/since_v0_9_0.rs`, chuyển `latest` sang 0.9.0 trong `wit.rs` | P1-3 | 0.5d | Agent B | ✅ |
| P2-2 | Variant `V0_9_0`, `call_panel_render`, `call_panel_handle_event` (version cũ trả lỗi rõ ràng) | P2-1 | 0.5d | Agent B | ✅ |
| P2-3 | Thêm `panel_render` / `panel_handle_event` vào trait `Extension` (`crates/extension/src/extension.rs`) và impl cho `WasmExtension` | P2-2 | 0.5d | Agent B | ✅ |
| P2-4 | Implement import `ui::request-render` qua `on_main_thread` | P2-1, P3-4 | 0.5d | Agent B | ✅ |

## Phase 3 — Manifest + proxy + registry

| ID | Task | Phụ thuộc | Ước lượng | Người làm | Trạng thái |
|---|---|---|---|---|---|
| P3-1 | `PanelManifestEntry` + field `panels` trong `ExtensionManifest` | P0-1 | 0.3d | Lead | ✅ |
| P3-2 | Bỏ qua `panels` + log warning nếu version API < 0.9.0 | P3-1, P2-2 | 0.2d | Agent C | ✅ |
| P3-3 | Trait `ExtensionPanelProxy` trong `extension_host_proxy.rs` | P3-1 | 0.3d | Lead | ✅ |
| P3-4 | `ExtensionPanelRegistry` (global) implement proxy | P3-3, P4a-4 | 0.5d | Agent A | ✅ |
| P3-5 | Gắn panel vào workspace đang mở + `observe_new::<Workspace>` | P3-4 | 0.5d | Agent A | ✅ |
| P3-6 | Gọi register/unregister trong `extension_host.rs` khi load/unload/reload; gọi `extension_panel::init` khi khởi động | P3-4 | 0.5d | Agent C | ✅ |

## Phase 4b — Renderer

| ID | Task | Phụ thuộc | Ước lượng | Người làm | Trạng thái |
|---|---|---|---|---|---|
| P4b-1 | Map `v-stack`, `h-stack`, `label`, `divider` | P4a-4 | 0.5d | Agent A | ✅ |
| P4b-2 | Map `button`, `checkbox` | P4b-1 | 0.5d | Agent A | ✅ |
| P4b-3 | Map `list-item` (indent, icon, selected, expanded) + whitelist icon | P4b-1 | 1d | Agent A | ✅ |
| P4b-4 | `text-input` giữ `Entity<Editor>` theo `id`, không mất focus/cursor khi re-render | P4b-1 | 1d | Agent A | ✅ |

## Phase 5 — Event loop + ổn định

| ID | Task | Phụ thuộc | Ước lượng | Người làm | Trạng thái |
|---|---|---|---|---|---|
| P5-1 | Event → `panel_handle_event` → `panel_render` → cập nhật cache → `cx.notify()` | P2-3, P4b-2 | 0.5d | Agent A | ✅ |
| P5-2 | Gộp render bằng cờ `dirty` | P5-1 | 0.5d | Agent A | ✅ |
| P5-3 | Validate arena (index, chu trình, root, giới hạn node/string) | P5-1 | 0.5d | Lead | ✅ |
| P5-4 | Error state trong panel + nút Retry | P5-1 | 0.5d | Agent A | ✅ |
| P5-5 | Đo độ trễ của vòng event, ghi kết quả vào tài liệu thiết kế | P5-2 | 0.5d | | ✅ |

## Phase 6 — Extension mẫu + test

| ID | Task | Phụ thuộc | Ước lượng | Người làm | Trạng thái |
|---|---|---|---|---|---|
| P6-1 | `extensions/test-panel`: counter, todo list, tree giả lập | P1-5, P3-6 | 1d | Agent C | ✅ |
| P6-2 | Unit test: builder flatten, validate arena | P1-5, P5-3 | 0.3d | Agent B | ✅ |
| P6-3 | Unit test: parse manifest có/không có `panels` | P3-1 | 0.2d | Agent C | ✅ |
| P6-4 | GPUI test: click → cây mới được render | P5-1 | 0.3d | Agent A | ✅ |
| P6-5 | GPUI test: gộp render khi có event liên tiếp | P5-2 | 0.3d | Agent A | ✅ |
| P6-6 | Integration test: load/unload `test-panel` | P6-1 | 0.5d | Agent C | ✅ |
| P6-7 | Compatibility test: extension 0.8.0 vẫn chạy | P2-2 | 0.2d | Agent C | ✅ |
| P6-8 | `./script/clippy` sạch + chạy lại toàn bộ test | P6-1…P6-7 | 0.2d | | ✅ |

## Phase 8 — Widget mở rộng + Git API + extension Source Control

Thiết kế: mục 4.1 và 13 của [extension-ui-api.md](./extension-ui-api.md). Đã chốt: dữ liệu lấy qua interface `git` của host (GitStore), sửa trực tiếp 0.9.0 vì chưa publish, phạm vi là phần lõi của Source Control view trong VS Code.

| ID | Task | Phụ thuộc | Trạng thái |
|---|---|---|---|
| P8-1 | `panel-instance` truyền vào `panel-render` / `panel-handle-event`; mỗi `ExtensionPanel` có instance riêng | P6 | ✅ |
| P8-2 | Widget mới: `icon-button`, `spacer`, `label` có style, `button` có icon/style/full-width/tooltip, `text-input` multi-line, `list-item` có description/decoration/actions/tooltip/icon file, `stack` có `gap` | P8-1 | ✅ |
| P8-3 | Renderer cho widget mới, icon theo icon theme, hover action, ctrl-enter / cmd-enter để submit multi-line | P8-2 | ✅ |
| P8-4 | `git.wit` + guest module `zed_extension_api::git` | P8-1 | ✅ |
| P8-5 | Capability `git` (manifest, settings, mặc định được cấp) | P8-4 | ✅ |
| P8-6 | `ExtensionGitProxy` + implement bằng GitStore (`extension_panel/src/git_proxy.rs`), giới hạn theo extension sở hữu instance, render lại khi Git thay đổi | P8-4 | ✅ |
| P8-7 | `extensions/source-control` | P8-3, P8-6 | ✅ |
| P8-8 | Test: builder, validate action, granter, git proxy (GPUI) | P8-6 | ✅ |
| P8-9 | Chạy thử trong Zed thật với một repository | P8-7 | ⬜ |

## Phase 9 — Hoàn thiện Source Control so với VS Code

Các tính năng còn thiếu ở mục 13.6 của tài liệu thiết kế, làm theo thứ tự dưới đây.

| ID | Task | Phụ thuộc | Trạng thái |
|---|---|---|---|
| P9-1 | Export `panel-release`: host báo khi một instance panel bị hủy để extension dọn state | P8 | ✅ |
| P9-2 | Badge trên icon dock: `ui-tree.badge` → `Panel::icon_label` | P8 | ✅ |
| P9-3 | Widget menu: `menu-entry` (item / separator / header); context menu chuột phải cho `list-item` | P8 | ✅ |
| P9-4 | `button.menu` → split button có dropdown; `icon-button.menu` → nút mở menu | P9-3 | ✅ |
| P9-5 | `git.wit`: `fetch`, `pull`, `push`, `branches`, `checkout-branch`, `create-branch`, `stash-all`, `stash-pop`, `init-repository` | P8 | ✅ |
| P9-6 | `open-diff` theo section: staged = HEAD ↔ index, changes = index ↔ working tree | P8 | ✅ |
| P9-7 | Danh sách lớn: panel render bằng `gpui::list` (chỉ layout dòng đang hiện), nâng giới hạn node lên 20 000, extension cắt bớt khi vượt giới hạn | P8 | ✅ |
| P9-8 | Nút hover nằm đè lên cuối dòng thay vì chiếm chỗ cố định | P8 | ✅ |
| P9-9 | Extension Source Control dùng toàn bộ phần trên: context menu, dropdown Commit, menu "…", nút Sync, chọn/tạo branch, stash, init, badge, dọn state | P9-1…P9-8 | ✅ |
| P9-10 | Test + clippy + cập nhật tài liệu thiết kế + chạy thử trong Zed | P9-9 | ✅ |

## Phase 10 — Gợi ý commit message bằng AI

Đã chốt: dùng model AI người dùng đã cấu hình trong Zed (hướng A), chạy bất đồng bộ để panel không bị đứng.

| ID | Task | Phụ thuộc | Trạng thái |
|---|---|---|---|
| P10-1 | Tách `commit_message_request` / `generate_commit_message_text` ra khỏi `GitPanel` (Git panel vẫn dùng như cũ) | P9 | ✅ |
| P10-2 | Event `task-completed` trong `ui-event`: kết quả của thao tác dài do host gửi về sau | P9 | ✅ |
| P10-3 | `git::generate-commit-message(instance, repository, task-id)` qua `ExtensionGitProxy` | P10-1, P10-2 | ✅ |
| P10-4 | Extension: nút ✨ cạnh ô message, trạng thái "Generating…", hiện lỗi khi chưa cấu hình model | P10-3 | ✅ |
| P10-5 | Test: sinh message với fake model, lỗi khi không có model; test `git_ui` không đổi; chạy thử trong Zed | P10-4 | 🟦 |

## Ghi chú kiểm chứng (2026-10-04)

- `cargo check`: `workspace`, `extension`, `extension_host`, `extension_panel` sạch.
- `cargo test`: `extension` 20/20, `extension_panel` 3/3, `extension_host` 49/49, `workspace` 260/260.
- `CC=clang CXX=clang++ ./script/clippy -p extension -p zed_extension_api -p extension_host -p extension_panel -p workspace -p extensions_ui -p extension_cli -p zed_test_panel -p zed` sạch (release, `--all-targets --all-features --deny warnings`). Chưa chạy cho toàn workspace; `cargo-shear`/`typos` chưa cài trên máy nên script bỏ qua các bước đó.
- `cargo check -p zed` sạch (build bằng clang: `CC=clang CXX=clang++`; cần `libasound2-dev`, `clang`, `libstdc++-12-dev`).
- P4a-5: test `test_panels_of_same_type_are_independent`. Kích thước persist theo `panel_key` riêng của từng panel; vị trí dock hiện chỉ giữ trong bộ nhớ, chưa persist qua lần khởi động lại.
- P6-6: test `test_extension_store_with_test_panel` build `test-panel` ra wasm, đăng ký panel, render → event → render lại, uninstall → unregister.
- P6-7: `zed_extension_api` 0.8.0 chưa từng publish lên crates.io nên chưa có test cố định. Đã kiểm tra thủ công: build `test-extension` từ `HEAD` với API 0.8.0 rồi chạy `test_extension_store_with_test_extension` trên guest đó → pass.
- P5-5: vòng click → render p50 108 µs, p99 927 µs (cây 15 node, release). Chi tiết ở mục 3.2 tài liệu thiết kế.
- Còn lại: P0-2 (review với team).

## Thứ tự làm đề xuất

```
P0 → P4a (spike) ─┬─► P3-4 → P3-5 → P3-6 ─┐
                  └─► P4b ────────────────┤
P1 → P2-1 → P2-2 → P2-3 ──────────────────┼─► P5 → P6
P3-1 → P3-3 ──────────────────────────────┘
```

P1/P2 và P4a có thể làm song song nếu có 2 người.

## Sau MVP (backlog)

| ID | Task | Trạng thái |
|---|---|---|
| B-1 | `virtual-list` + export `panel-render-items` (dùng `uniform_list`) | 🟨 Layout đã ảo hóa (P9-7), cây vẫn gửi trọn |
| B-2 | Context menu cho node | ✅ (P9-3) |
| B-3 | Keybinding/action riêng của extension | ⬜ |
| B-4 | Mở file/diff từ panel | ✅ (qua `git::open-file` / `git::open-diff`, P8) |
| B-5 | Interface `git` (status, diff, stage) có capability gating | ✅ (P8) |
| B-6 | Use case: Source Control extension | ✅ (P8) |
| B-7 | Export báo instance bị hủy để extension dọn state | ✅ (P9-1) |
| B-8 | Pull / Push / Sync, chuyển branch trong `git.wit` | ✅ (P9-5) |
| B-9 | Badge số thay đổi trên icon dock (`Panel::icon_label`) | ✅ (P9-2) |
| B-10 | Force push có xác nhận ở host | ⬜ |
| B-11 | Modal / quick pick cho extension | ⬜ |
| B-12 | Gửi cây theo từng phần (diff cây) để giảm chi phí với repository rất lớn | ⬜ |
