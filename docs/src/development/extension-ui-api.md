# Extension UI API — Tài liệu phát triển

> Trạng thái: **Draft / MVP** · Phạm vi: `zed_extension_api` 0.9.0 (Dev/Nightly)

## 1. Bối cảnh

Zed có UI framework đầy đủ (GPUI), nhưng Extension API hiện tại (`zed_extension_api` 0.8.0) chỉ cho extension đóng góp:

- Language, grammar, LSP
- Slash command, context server, debug adapter
- Theme, icon theme, snippet

Extension **không thể** tạo panel, view, sidebar hay bất kỳ UI tương tác nào. Vì vậy các tính năng như Source Control kiểu VS Code, Database Browser, API Client không thể viết dưới dạng extension.

Mục tiêu không phải là làm riêng một tính năng "Source Control giống VS Code", mà là **xây một UI Extension API** để extension tự dựng UI tương tác. Source Control chỉ là một use case trên nền API đó.

```
              UI Extension API
                     │
       ┌─────────────┼─────────────┐
       │             │             │
 Source Control   Database      API Client
                  Browser
```

### Vì sao upstream chưa làm

| Vấn đề | Hệ quả với thiết kế |
|---|---|
| GPUI là implementation nội bộ | Không expose GPUI; extension chỉ mô tả UI bằng dữ liệu |
| Extension chạy trong WASM sandbox | Mọi giao tiếp đi qua WIT, lời gọi là async |
| Performance | Giới hạn kích thước cây UI, gộp render, virtual list ở phase sau |
| Security | Chỉ cho phép widget trong whitelist, không có quyền vẽ trực tiếp |
| API compatibility | WIT versioned (`since_v0.9.0`), schema widget ổn định, độc lập với GPUI |

## 2. Mục tiêu

### Mục tiêu MVP

1. Extension khai báo panel trong `extension.toml`.
2. Panel xuất hiện trên dock, có icon và có toggle được.
3. Extension render UI bằng các widget cơ bản: stack, label, button, text input, checkbox, list item (dạng tree qua `indent`), divider.
4. Event (click, input, toggle) được gửi vào WASM; extension cập nhật state và UI re-render.
5. Extension cũ (≤ 0.8.0) vẫn chạy bình thường.

### Ngoài phạm vi MVP

- Virtual list cho danh sách lớn (Phase 7)
- Context menu, keybinding riêng của extension
- Custom view trong pane (editor tab)
- DB host API cho các use case cụ thể (Git API đã có, xem mục 13)
- Lưu state qua lần reload extension

## 3. Kiến trúc

### 3.1 Mô hình: declarative UI kiểu Elm

State nằm trong WASM; host chỉ render từ một cây UI đã cache.

```
Extension (WASM)                          Zed host (GPUI)
────────────────                          ───────────────
state (Rust struct)                       ExtensionPanel (impl Panel)
                                          cache: UiTree
panel-render(id) -> ui-tree     ◄──call── sau event / khi được yêu cầu
panel-handle-event(id, event)   ◄──call── click / input / toggle
request-render(id)              ──import─► spawn call → cập nhật cache → cx.notify()
```

Lý do chọn mô hình này:

- **Không lộ GPUI.** Extension chỉ trả về dữ liệu (`ui-tree`), host map sang component của crate `ui` (`Label`, `Button`, `ListItem`, ...). GPUI có thay đổi thì WIT vẫn giữ nguyên.
- **Đúng ràng buộc async.** Lời gọi WASM chạy qua `WasmExtension::call` (`crates/extension_host/src/wasm_host.rs`) và là async, còn `Render::render` của GPUI là sync. Vì vậy host **không bao giờ gọi WASM bên trong `render`**, mà chỉ đọc cây đã cache.
- **An toàn.** Tập widget là whitelist. Extension không truy cập được entity, window hay filesystem qua UI API.

### 3.2 Luồng một event

```
User click button "increment"
  │
  ▼
ExtensionPanel::on_click(id)                       [main thread]
  │  nếu đang có call chạy → đánh dấu dirty, return
  ▼
cx.spawn: extension.call(panel_handle_event(id, Clicked("increment")))
  │                                                [wasm thread]
  ▼
extension.call(panel_render(id)) -> UiTree
  │
  ▼
this.update: validate tree → self.tree = Some(tree) → cx.notify()
  │  nếu dirty → chạy lại vòng render một lần nữa
  ▼
GPUI render() đọc self.tree → element
```

#### Kết quả đo độ trễ (P5-5, 2026-10-04)

Đo phần phía extension của một vòng event, tức phần `ExtensionPanel` phải chờ trước khi `cx.notify()`: `panel_handle_event` → `panel_render` → `UiTree::validate`. Đo trên `extensions/test-panel` (cây 15 node), build `--release`, gọi qua `WasmExtension` thật (wasmtime, message loop trên tokio), chạy nóng 50 lần rồi đo 1000 lần mỗi loại. Máy: AMD Ryzen 7 5700U, đang chịu tải build song song.

| Thao tác | p50 | p90 | p99 | max |
|---|---|---|---|---|
| `panel_render` + `validate` | 60 µs | 87 µs | 258 µs | 3.0 ms |
| click: `panel_handle_event` + `panel_render` + `validate` | 108 µs | 164 µs | 927 µs | 3.5 ms |

Kết luận: với cây cỡ nhỏ, một vòng event mất dưới 1 ms ở p99, nhỏ hơn nhiều so với một frame 60 Hz (16.7 ms). Chưa cần gộp `panel-handle-event` và `panel-render` thành một lần gọi. Cần đo lại khi có cây lớn (hàng nghìn node, ví dụ Source Control) vì chi phí copy cây qua ranh giới WASM tăng theo số node.

Cách đo: tạm thêm vòng lặp đo vào `test_extension_store_with_test_panel` rồi chạy `cargo test --release -p extension_host test_extension_store_with_test_panel -- --nocapture`. Benchmark criterion có sẵn (`benches/extension_compilation_benchmark.rs`) chưa dùng được vì `load_extension` bị treo trên test scheduler của bench, kể cả với benchmark `load` gốc.

### 3.3 Các thành phần

| Thành phần | Crate | Vai trò |
|---|---|---|
| WIT `ui` interface | `extension_api/wit/since_v0.9.0` | Hợp đồng giữa extension và host |
| Guest API + builder | `extension_api` | Trait methods, `ui::*` builder, `request_render` |
| Host binding | `extension_host/src/wasm_host/wit/since_v0_9_0.rs` | Gọi export và implement import |
| Manifest | `extension/src/extension_manifest.rs` | Field `panels` |
| Proxy | `extension/src/extension_host_proxy.rs` | `ExtensionPanelProxy` để tách `extension_host` khỏi `workspace` |
| `ExtensionPanel` | crate mới `extension_panel` | GPUI view, registry, renderer |

`extension_host` không phụ thuộc `workspace`, nên việc thêm panel vào workspace phải đi qua một proxy trait. Cách này giống pattern `ExtensionContextServerProxy` và `ExtensionLanguageModelProviderProxy` đang có.

## 4. Thiết kế API

### 4.1 WIT — `crates/extension_api/wit/since_v0.9.0/ui.wit`

WIT **không hỗ trợ kiểu đệ quy**, nên cây UI được biểu diễn dạng flat arena: một `list<node>` cộng với index của các node con. Bản đầy đủ có doc comment nằm trong file WIT; dưới đây là phần rút gọn.

```wit
interface ui {
    type node-id = u32;
    /// Mỗi workspace có một instance riêng của mỗi panel.
    type panel-instance = u64;

    enum color { default, muted, accent, success, warning, error, created, modified, deleted, conflict, ignored }
    enum label-size { default, small, x-small }
    enum spacing { none, small, medium, large }
    enum button-style { default, filled, subtle }

    record stack { children: list<node-id>, gap: spacing }
    record label { text: string, color: color, size: label-size, bold: bool, strikethrough: bool }
    variant icon { named(string), file(string), folder(string) }

    record button {
        id: string, label: string, disabled: bool,
        icon: option<string>, style: button-style, full-width: bool, tooltip: option<string>,
        menu: list<menu-entry>,         // không rỗng → có mũi tên dropdown ở cuối nút
    }
    record menu-item { id: string, label: string, icon: option<string>, disabled: bool, checked: option<bool> }
    variant menu-entry { item(menu-item), separator, header(string) }

    record icon-button {
        id: string, icon: string, tooltip: option<string>, disabled: bool,
        menu: list<menu-entry>,         // không rỗng → bấm nút thì mở menu
    }
    record text-input { id: string, placeholder: string, value: string, multi-line: bool }
    record checkbox { id: string, label: string, checked: bool }
    record decoration { text: string, color: color }

    record list-item {
        id: string, label: string, indent: u32, icon: option<icon>, selected: bool,
        /// `none` = node lá; `some(true/false)` = node có thể mở/đóng.
        expanded: option<bool>,
        description: option<string>,   // chữ mờ sau label, ví dụ thư mục chứa file
        label-color: color,
        strikethrough: bool,
        decoration: option<decoration>, // chữ ở cuối dòng, ví dụ `M`
        actions: list<icon-button>,     // nút hiện khi hover, đè lên cuối dòng
        tooltip: option<string>,
        context-menu: list<menu-entry>, // menu chuột phải
    }

    variant node {
        v-stack(stack), h-stack(stack),
        label(label), button(button), icon-button(icon-button),
        text-input(text-input), checkbox(checkbox), list-item(list-item),
        divider, spacer,
    }

    record ui-tree {
        nodes: list<node>, root: node-id,
        badge: option<u32>,             // số hiện trên icon panel ở dock khi panel đang đóng
    }

    variant ui-event {
        clicked(string),               // button, icon-button, list-item, action hoặc menu item
        input-changed(input-event),
        input-submitted(input-event),  // enter; với multi-line là ctrl-enter / cmd-enter
        checkbox-toggled(checkbox-event),
        list-item-toggled(string),
        task-completed(task-result),   // kết quả của thao tác dài, ví dụ sinh commit message
    }

    record task-result { id: string, output: result<string, string> }

    /// Render lại mọi instance của panel.
    request-render: func(panel-id: string);
}
```

Phần bổ sung vào `world extension` trong `extension.wit`:

```wit
import git;
import ui;
use ui.{ui-tree, ui-event, panel-instance};

export panel-render: func(panel-id: string, instance: panel-instance) -> result<ui-tree, string>;
export panel-handle-event: func(panel-id: string, instance: panel-instance, event: ui-event) -> result<_, string>;
/// Gọi khi một instance bị hủy (đóng window, gỡ extension) để extension dọn state.
export panel-release: func(panel-id: string, instance: panel-instance);
```

**Quy ước `id`:** mỗi node tương tác có một `id` ổn định qua các lần render. Host dùng `id` để định tuyến event và để giữ state phía host như focus hay `Entity<Editor>` của text input. Action của list item phát `clicked` với `id` của action, không phải của item.

**`panel-instance`:** tất cả window dùng chung một instance WASM của extension, nên host truyền `instance` vào mỗi lần gọi để extension biết đang render cho window nào và để các host API như `git` biết phải thao tác trên project nào. Extension nên lưu state theo `instance`.

### 4.2 Guest API (Rust)

Trait `zed::Extension` có thêm hai method với default impl, nên extension cũ không phải sửa:

```rust
fn panel_render(&mut self, panel_id: &str, instance: ui::PanelInstance) -> Result<ui::Tree> {
    Err(format!("panel `{panel_id}` is not implemented"))
}

fn panel_handle_event(
    &mut self,
    panel_id: &str,
    instance: ui::PanelInstance,
    event: ui::Event,
) -> Result<()> {
    Ok(())
}
```

Builder `ui::*` flatten cây lồng nhau thành arena:

```rust
ui::v_stack()
    .gap(ui::Spacing::None)
    .child(ui::label("Changes").size(ui::LabelSize::Small).bold(true))
    .child(
        ui::list_item("file:src/main.rs", "main.rs")
            .icon(ui::Icon::File("src/main.rs".into()))
            .description("src")
            .decoration("M", ui::Color::Modified)
            .action(ui::action("stage:src/main.rs", "plus").tooltip("Stage Changes")),
    )
    .child(ui::button("commit", "Commit").style(ui::ButtonStyle::Filled).full_width(true))
    .build() // -> ui::Tree { nodes, root }
```

### 4.3 Manifest — `extension.toml`

```toml
schema_version = 1

[panels.counter]
title = "Counter"
icon = "file_tree"           # map vào whitelist IconName; không hợp lệ → icon mặc định
default_position = "left"    # left | right | bottom
```

Rust:

```rust
#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize)]
pub struct PanelManifestEntry {
    pub title: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub default_position: PanelPosition,
}
```

## 5. Triển khai phía host

### 5.1 Danh sách file cần thay đổi

| File | Thay đổi |
|---|---|
| `crates/extension_api/wit/since_v0.9.0/*` | Copy từ `since_v0.8.0`, thêm `ui.wit`, cập nhật `extension.wit` |
| `crates/extension_api/src/extension_api.rs` | Trait methods, module `ui`, `request_render` |
| `crates/extension_api/Cargo.toml` | `version = "0.9.0"` |
| `crates/extension_host/src/wasm_host/wit.rs` | `mod since_v0_9_0`, `use since_v0_9_0 as latest`, variant `V0_9_0`, `call_panel_render`, `call_panel_handle_event` |
| `crates/extension_host/src/wasm_host/wit/since_v0_9_0.rs` | Binding mới, implement import `ui::request-render` |
| `crates/extension/src/extension_manifest.rs` | `panels: BTreeMap<Arc<str>, PanelManifestEntry>` |
| `crates/extension/src/extension.rs` | `panel_render`, `panel_handle_event` trong trait `Extension` |
| `crates/extension/src/extension_host_proxy.rs` | `ExtensionPanelProxy` |
| `crates/extension_host/src/extension_host.rs` | Register/unregister panel khi load, unload hoặc reload |
| `crates/extension_panel/` (mới) | `ExtensionPanel`, registry, renderer |
| `crates/workspace/src/dock.rs` | Hỗ trợ tên panel động (xem 5.3) |
| `crates/zed/src/main.rs` (hoặc nơi init) | Gọi `extension_panel::init(cx)` |

### 5.2 Version gating

`crates/extension_host/src/wasm_host/wit.rs` đã có cơ chế giới hạn version theo release channel:

```rust
ReleaseChannel::Dev | ReleaseChannel::Nightly => latest::MAX_VERSION,
ReleaseChannel::Stable | ReleaseChannel::Preview => since_v0_6_0::MAX_VERSION,
```

Với version 0.9.0, chỉ cần đổi `latest` thành `since_v0_9_0`. Stable không bị ảnh hưởng. Với các variant cũ, `call_panel_render` trả `Err(anyhow!("panels require zed_extension_api 0.9.0+"))`.

### 5.3 Trở ngại trong `Panel` trait

Trait `Panel` (`crates/workspace/src/dock.rs`) được thiết kế cho **mỗi type Rust tương ứng một panel**. Extension panel thì ngược lại: **một type `ExtensionPanel` có nhiều instance**.

| Vấn đề | Vị trí | Hướng xử lý |
|---|---|---|
| `fn persistent_name() -> &'static str` và `fn panel_key() -> &'static str` là hàm static | `dock.rs` (trait `Panel`); blanket impl `PanelHandle` gọi `T::persistent_name()` | Thêm method `&self` vào `Panel` với default impl gọi hàm static, để `PanelHandle` dùng method này và `ExtensionPanel` override được. Phương án tạm cho spike: `Box::leak` tên lúc register (số lượng giới hạn theo số panel) |
| `workspace.panel::<T>()` trả panel **đầu tiên** cùng type | `workspace.rs` | Viết helper riêng: duyệt các dock, downcast sang `ExtensionPanel`, lọc theo `panel_id` |
| `icon()` trả về enum cố định `IconName` | `dock.rs` | Map string sang `IconName` qua whitelist |
| `icon_tooltip()` trả về `Option<&'static str>` | `dock.rs` | Xử lý giống `persistent_name` |
| `toggle_action()` cần một `Box<dyn Action>` | `dock.rs` | Action có data: `ToggleExtensionPanel { panel_id: String }` |
| Persist vị trí/kích thước theo `persistent_name` | `dock.rs`, `workspace.rs` | Đặt tên dạng `extension:{extension_id}:{panel_id}` để không đụng tên panel built-in |

> Đây là rủi ro lớn nhất của dự án. Nên làm spike phần này trước (Phase 4a) bằng một cây UI giả, chưa cần WASM.

### 5.4 `ExtensionPanel`

```rust
pub struct ExtensionPanel {
    extension: Arc<dyn Extension>,
    panel_id: Arc<str>,
    manifest_entry: PanelManifestEntry,
    tree: Option<UiTree>,
    error: Option<SharedString>,
    inputs: HashMap<SharedString, Entity<Editor>>,
    render_task: Option<Task<()>>,
    dirty: bool,
    focus_handle: FocusHandle,
    position: DockPosition,
}
```

Nguyên tắc:

- `render()` chỉ đọc `self.tree` và `self.error`. Khi chưa có cây thì hiện trạng thái loading.
- Mỗi `text-input` giữ một `Entity<Editor>` theo `id`. Khi re-render chỉ cập nhật text nếu khác giá trị hiện tại, để không làm mất cursor hay focus. Input bị xóa khỏi cây thì dọn entity tương ứng.
- **Gộp request render:** nếu `render_task` đang chạy thì chỉ đặt `dirty = true`. Khi task xong, nếu `dirty` thì chạy lại đúng một lần.
- Lỗi từ WASM được lưu vào `self.error` và hiển thị trong panel kèm nút "Retry". Không panic, không `unwrap()`.

### 5.4.1 Danh sách ảo hóa

Panel hiển thị cây bằng `gpui::list`, nên chỉ layout các dòng đang nhìn thấy. Mỗi con của v-stack gốc là một dòng. Riêng v-stack con có `gap = none` và chỉ chứa list item thì được trải phẳng, mỗi list item thành một dòng (`list_rows`). Khi cây mới về, host dùng `ListState::splice` thay cho `reset` để giữ vị trí cuộn.

### 5.5 Registry và việc gắn panel vào workspace

- Global `ExtensionPanelRegistry` implement `ExtensionPanelProxy`.
- `register_panel`: lưu entry, rồi duyệt các workspace đang mở để `add_panel`.
- Workspace mới: `cx.observe_new::<Workspace>` đọc registry và thêm panel.
- `unregister_panel` (khi unload/reload extension): gỡ panel khỏi mọi dock.

### 5.6 `request-render` từ WASM

Host import chạy trên thread WASM. Nó dùng `on_main_thread` (`wasm_host.rs`) để đưa tín hiệu về main thread, tìm `ExtensionPanel` theo `(extension_id, panel_id)` rồi kích hoạt vòng render như ở mục 3.2.

## 6. Bảo mật và giới hạn

| Giới hạn | Giá trị đề xuất | Lý do |
|---|---|---|
| Số node tối đa mỗi cây | 20 000 | Panel chỉ layout các dòng đang hiện (mục 5.7), nên giới hạn này chủ yếu chặn chi phí gửi cây qua WASM |
| Độ dài string tối đa (label, value) | 64 KiB | Tránh layout quá nặng |
| Validate arena | Index con phải hợp lệ, không có chu trình, `root` tồn tại | Dữ liệu từ WASM là untrusted |
| Số action tối đa mỗi list item | 8 | Action nằm trong list item chứ không phải node riêng, nên không bị giới hạn số node chặn |
| Số entry tối đa mỗi menu | 100 | Như trên |
| Widget | Chỉ whitelist ở mục 4.1 | Không có raw drawing, HTML hay script |
| Icon | Whitelist `IconName` | Không load asset tùy ý |

Validate được thực hiện **trước** khi gán vào `self.tree`. Nếu cây không hợp lệ thì giữ cây cũ và hiển thị lỗi.

## 7. Compatibility

- WIT mới đặt trong thư mục riêng `since_v0.9.0`. Các thư mục cũ không bị sửa.
- Extension ≤ 0.8.0 không thể khai báo `panels`. Nếu manifest có `panels` mà version API thấp hơn 0.9.0 thì bỏ qua và log warning.
- Khi thêm widget hay event mới thì tăng minor version và thêm case vào `variant`. Không đổi nghĩa của case đã có.
- Schema widget là public contract. Tên field phải mô tả ý nghĩa, không phản ánh chi tiết GPUI.

## 8. Lộ trình

| Phase | Nội dung | Kết quả | Ước lượng |
|---|---|---|---|
| 0 | Branch, setup | Branch `extension-ui-api` | 0.5 ngày |
| 4a | **Spike** `Panel` trait + `ExtensionPanel` với cây giả | Panel động hiện trên dock, toggle và persist được | 3–4 ngày |
| 1 | WIT 0.9.0 + guest API + builder | `zed_extension_api` 0.9.0 build được | 2–3 ngày |
| 2 | Host binding `since_v0_9_0.rs` | Gọi được `panel-render` từ host | 2 ngày |
| 3 | Manifest + `ExtensionPanelProxy` + register/unregister | Panel tự xuất hiện khi cài extension | 2 ngày |
| 4b | Renderer đầy đủ + text input | Toàn bộ widget MVP render đúng | 3 ngày |
| 5 | Event loop, gộp render, validate, error state | Tương tác ổn định | 3 ngày |
| 6 | Extension mẫu + test + clippy | `extensions/test-panel`, test xanh | 3 ngày |
| 7 | Sau MVP: virtual list, context menu, git API | Nền tảng cho Source Control | — |

Tổng MVP: khoảng 3–4 tuần cho một developer quen codebase.

### Checklist chi tiết

**Phase 4a — Spike**
- [ ] Thêm method động thay cho `persistent_name`/`panel_key`/`icon_tooltip` vào `Panel`, giữ default impl cho panel hiện có
- [ ] Action `ToggleExtensionPanel { panel_id }`
- [ ] `ExtensionPanel` render một `UiTree` hardcode
- [ ] Thêm 2 panel cùng type vào cùng workspace, kiểm tra toggle và persist vị trí độc lập

**Phase 1–2**
- [ ] Copy `since_v0.8.0` → `since_v0.9.0`, thêm `ui.wit`
- [ ] Guest: trait methods, `ui` builder, `request_render`
- [ ] Host: `since_v0_9_0.rs`, variant `V0_9_0`, `call_panel_*`
- [ ] Chuyển `latest` sang `since_v0_9_0`

**Phase 3**
- [ ] `PanelManifestEntry` + parse test
- [ ] `ExtensionPanelProxy` + wiring trong `extension_host.rs`
- [ ] Registry, `observe_new::<Workspace>`, gỡ panel khi unload

**Phase 4b–5**
- [ ] Renderer cho từng widget
- [ ] Text input giữ `Entity<Editor>` theo `id`
- [ ] Event → `panel_handle_event` → `panel_render`
- [ ] Gộp render (`dirty`), validate arena, giới hạn kích thước
- [ ] Error state + Retry

**Phase 6**
- [ ] `extensions/test-panel`: counter, todo list, tree giả lập
- [ ] Test (mục 9)
- [ ] `./script/clippy`

## 9. Testing

| Loại | Nội dung |
|---|---|
| Unit | Builder flatten đúng arena; validate từ chối index sai, chu trình, cây quá lớn |
| Unit | Parse manifest có và không có `panels` |
| GPUI test | Click → event được gửi → cây mới được render (`cx.run_until_parked()`) |
| GPUI test | Hai event liên tiếp khi task đang chạy chỉ dẫn tới thêm đúng một lần render |
| GPUI test | Hai panel cùng type: toggle và persist độc lập |
| Integration | Load `test-panel`, kiểm tra panel có trên dock; unload thì panel biến mất |
| Compatibility | Extension 0.8.0 vẫn load; gọi `panel_render` trên đó trả lỗi rõ ràng |

Trong GPUI test, dùng `cx.background_executor.timer(...)` thay cho `smol::Timer::after(...)`.

## 10. Tiêu chí nghiệm thu MVP

- [ ] Extension mẫu khai báo panel trong `extension.toml`, panel hiện trên dock với đúng icon và title
- [ ] Toggle panel qua icon và qua action
- [ ] Click, gõ input, tick checkbox, mở/đóng tree item đều làm state trong WASM thay đổi và UI cập nhật
- [ ] Text input không mất focus/cursor khi re-render
- [ ] Lỗi trong extension hiện trong panel, Zed không crash
- [ ] Reload hoặc uninstall extension thì panel biến mất sạch
- [ ] Extension ≤ 0.8.0 không bị ảnh hưởng
- [ ] Clippy và test đều xanh

## 11. Rủi ro và câu hỏi mở

| Rủi ro / câu hỏi | Ảnh hưởng | Hướng xử lý |
|---|---|---|
| Sửa `Panel` trait chạm tới nhiều panel built-in | Cao | Dùng default impl để panel cũ không phải sửa; spike trước |
| Độ trễ của vòng event → WASM → render | Thấp | Đã đo (mục 3.2): p99 < 1 ms với cây 15 node. Đo lại với cây lớn; chỉ gộp `panel-handle-event` + `panel-render` nếu cần |
| Cây lớn (ví dụ git status hàng nghìn file) | Cao với Source Control | Virtual list ở Phase 7 |
| Panel trong remote/collab session | Chưa rõ | MVP chỉ hỗ trợ local; `remote_id()` trả `None` |
| Có cần permission/capability riêng cho UI không | Thấp | MVP chỉ cần khai báo trong manifest; xem lại khi có API ghi dữ liệu |
| Upstream có thể làm API khác | Trung bình | Giữ thay đổi tách biệt (crate riêng, WIT riêng) để dễ rebase |

## 12. Phụ lục — Extension mẫu

`extensions/test-panel/extension.toml`

```toml
id = "test-panel"
name = "Test Panel"
version = "0.0.1"
schema_version = 1
authors = ["Zed Industries <support@zed.dev>"]
description = "Example extension panel"

[panels.counter]
title = "Counter"
icon = "file_tree"
default_position = "left"
```

`extensions/test-panel/src/test_panel.rs`

```rust
use zed_extension_api::{self as zed, ui};

struct TestPanelExtension {
    count: i32,
    todos: Vec<(String, bool)>,
    draft: String,
}

impl zed::Extension for TestPanelExtension {
    fn new() -> Self {
        Self {
            count: 0,
            todos: Vec::new(),
            draft: String::new(),
        }
    }

    fn panel_render(&mut self, _panel_id: &str, _instance: ui::PanelInstance) -> zed::Result<ui::Tree> {
        let mut root = ui::v_stack()
            .child(
                ui::h_stack()
                    .child(ui::label(format!("Count: {}", self.count)))
                    .child(ui::button("increment", "+1")),
            )
            .child(ui::divider())
            .child(ui::text_input("draft", "New todo…", &self.draft));

        for (index, (title, done)) in self.todos.iter().enumerate() {
            root = root.child(ui::checkbox(format!("todo-{index}"), title, *done));
        }

        Ok(root.build())
    }

    fn panel_handle_event(
        &mut self,
        _panel_id: &str,
        _instance: ui::PanelInstance,
        event: ui::Event,
    ) -> zed::Result<()> {
        match event {
            ui::Event::Clicked(id) if id == "increment" => self.count += 1,
            ui::Event::InputChanged { id, value } if id == "draft" => self.draft = value,
            ui::Event::InputSubmitted { id, value } if id == "draft" && !value.is_empty() => {
                self.todos.push((value, false));
                self.draft.clear();
            }
            ui::Event::CheckboxToggled { id, checked } => {
                if let Some(todo) = id
                    .strip_prefix("todo-")
                    .and_then(|index| index.parse::<usize>().ok())
                    .and_then(|index| self.todos.get_mut(index))
                {
                    todo.1 = checked;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

zed::register_extension!(TestPanelExtension);
```

## 13. Git API và extension Source Control

### 13.1 Interface `git` — `crates/extension_api/wit/since_v0.9.0/git.wit`

Extension không tự chạy `git`, mà gọi qua GitStore của Zed. Nhờ vậy dữ liệu khớp với Git panel có sẵn, thao tác đi qua hàng đợi job của Zed, và panel tự cập nhật khi repository thay đổi.

| Hàm | Kết quả | Ghi chú |
|---|---|---|
| `repositories(instance)` | `list<repository>` | `id`, `work-directory`, `branch`, `upstream` (ahead/behind), `active` |
| `status(instance, repository)` | `list<status-entry>` | `path` tương đối, dùng `/`; `file-status` = `untracked` / `ignored` / `conflicted` / `tracked { index, worktree }` |
| `stage` / `unstage(instance, repository, paths)` | `_` | |
| `stage-all` / `unstage-all(instance, repository)` | `_` | |
| `discard(instance, repository, paths)` | `bool` | Host hỏi xác nhận; file tracked được checkout từ `HEAD`, file chưa tồn tại trong `HEAD` được chuyển vào thùng rác. `false` nếu người dùng hủy |
| `commit(instance, repository, message, options)` | `_` | `options`: `amend`, `signoff`. Prompt của Git (ví dụ passphrase GPG) hiện trong modal |
| `open-diff(instance, repository, path, kind)` | `_` | `uncommitted`: `SoloDiffView` (diff một file, HEAD ↔ working tree); `staged`: `StagedDiff` (HEAD ↔ index); `unstaged`: `UnstagedDiff` (index ↔ working tree). Hai view sau là view nhiều file, cuộn tới file đó |
| `open-file(instance, repository, path)` | `_` | |
| `fetch(instance, repository)` | `_` | Fetch mọi remote |
| `pull(instance, repository, rebase)` / `push(instance, repository, force)` | `_` | Như Git panel: nhiều remote thì người dùng chọn; push tự set upstream khi branch chưa có |
| `branches(instance, repository)` | `list<branch>` | `name`, `is-head`, `is-remote`, `upstream` |
| `checkout-branch` / `create-branch(instance, repository, name)` | `_` | `create-branch` tạo từ `HEAD` rồi checkout |
| `stash-all` / `stash-pop(instance, repository)` | `_` | |
| `init-repository(instance)` | `_` | `git init` ở folder đầu tiên của project, branch mặc định `main` |
| `generate-commit-message(instance, repository, task-id)` | `_` | Bất đồng bộ: trả về ngay, sau đó panel nhận `task-completed { id: task-id, output }`. Dùng model commit message trong cài đặt AI của Zed và prompt của Git panel; mô tả thay đổi đã stage, nếu chưa stage gì thì mô tả mọi thay đổi. Lỗi ngay nếu AI bị tắt hoặc chưa cấu hình model |

**Thao tác dài.** Lời gọi WASM chặn extension cho tới khi trả về, nên trong lúc đó panel không render lại được. Thao tác có thể mất vài giây, như gọi LLM, sẽ trả về ngay sau khi bắt đầu, rồi host gửi kết quả về instance bằng event `task-completed` với `id` do extension tự đặt. Cùng cơ chế này có thể dùng cho pull/push sau này.

Guest API nằm trong module `zed_extension_api::git`, kèm helper `FileStatus::is_staged()` và `is_unstaged()`.

### 13.2 Bảo mật

- **Capability `git`.** Extension phải khai báo `[[capabilities]] kind = "git"` trong `extension.toml`, và host phải cấp nó trong `granted_extension_capabilities` (mặc định đã cấp). Kiểm tra ở `CapabilityGranter::grant_git`.
- **Giới hạn theo instance.** Mọi hàm nhận `instance`; host chỉ tìm instance trong các panel của **chính extension đang gọi**, nên extension không thể chạm tới project qua panel của extension khác.
- **Đường dẫn.** Mọi `path` đi qua `RepoPath::new`, hàm này từ chối đường dẫn tuyệt đối và `..` thoát ra ngoài repository.
- **Thao tác phá hủy.** `discard` luôn hỏi người dùng ở phía host, extension không thể bỏ qua bước này.

### 13.3 Cập nhật tự động

Lần đầu một instance gọi tới Git API, `ExtensionPanel` subscribe `GitStoreEvent` của project và render lại khi có `ActiveRepositoryChanged`, `RepositoryAdded`, `RepositoryRemoved`, hoặc `RepositoryUpdated` với `StatusesChanged` / `HeadChanged` / `BranchListChanged`. Panel không dùng Git thì không bị render lại. Các lần render liên tiếp được gộp bằng cờ `dirty` như mục 5.4.

### 13.4 Phía host

| Thành phần | File |
|---|---|
| Kiểu dữ liệu, `git::Operation` | `crates/extension/src/types/git.rs` |
| `ExtensionGitProxy` | `crates/extension/src/extension_host_proxy.rs` |
| Binding `git::Host` | `crates/extension_host/src/wasm_host/wit/since_v0_9_0.rs` |
| Implement proxy bằng GitStore | `crates/extension_panel/src/git_proxy.rs` |
| `GitStatusEntry::new` để mở `SoloDiffView` | `crates/git_ui/src/git_panel.rs` |

Multi-line text input dùng key context `ExtensionPanelTextArea`. Keymap map `ctrl-enter` (`cmd-enter` trên macOS) sang `menu::SecondaryConfirm`, và panel đổi action đó thành `input-submitted`. Cách này giống commit editor của Git panel, vì keybinding được xử lý trước listener `key_down`.

### 13.5 Extension `extensions/source-control`

Panel dựng theo Source Control view của VS Code:

```
SOURCE CONTROL                 [≡] [✓] [↻] [⋯]
┌ Message (Ctrl+Enter to commit on 'main') ┐
└──────────────────────────────────────────┘
[ ✓ Commit                              | ▾ ]   Commit / Commit (Amend) / Commit & Push / Commit & Sync
▾ Merge Changes                          [+] 1
    lib.rs  src                      [📄][+] !
▾ Staged Changes                         [−] 2
    main.rs  src                     [📄][−] M
▾ Changes                           [↶][+]   3
    new.rs  src                   [📄][↶][+] U
    old.rs  src                   [📄][↶][+] D   (gạch ngang)
```

- Icon panel ở dock có badge số thay đổi.
- Không có thay đổi mà branch lệch upstream thì nút Commit thành "Sync Changes 1↓ 2↑"; branch chưa có upstream thì thành "Publish Branch".
- Menu "⋯": Pull, Pull (Rebase), Push, Fetch, Checkout to…, Stash All Changes, Pop Latest Stash, View as List / Tree.
- "Checkout to…" mở picker ngay trong panel: gõ để lọc branch, Enter để checkout branch trùng tên, hoặc tạo branch mới nếu chưa có.
- Folder chưa có Git thì hiện nút "Initialize Repository".
- Nhiều repository thì hiện danh sách repository ở trên, kèm branch và ahead/behind.
- Bấm vào file thì mở diff: Staged Changes → HEAD ↔ index, Changes → index ↔ working tree, Merge Changes → HEAD ↔ working tree. Hover thì hiện Open File / Stage / Unstage / Discard, đè lên cuối dòng. Chuột phải mở context menu với các thao tác tương ứng.
- Có chế độ list và tree. Tree view gộp thư mục chỉ có một thư mục con, giống compact folders của VS Code, và có action theo thư mục.
- Commit: nếu chưa stage gì thì stage mọi thay đổi của file tracked rồi commit (smart commit). Không commit khi còn conflict.
- Mỗi section hiện tối đa 2 000 file, phần còn lại được ghi là "N more files not shown".
- Nút ✨ cạnh ô message sinh commit message bằng AI; trong lúc chờ panel vẫn dùng được và hiện "Generating commit message…".
- Lỗi của thao tác Git hiện ngay trong panel và có nút đóng, không thay thế cả panel.
- State lưu theo `instance` và được xóa trong `panel_release`.

### 13.6 Còn khác VS Code

| Điểm khác | Lý do |
|---|---|
| Smart commit chỉ stage file tracked, không hỏi trước | Theo Git panel của Zed, tránh commit nhầm file rác; VS Code mặc định stage cả file untracked sau khi hỏi |
| Diff staged / unstaged mở view nhiều file của Zed rồi cuộn tới file | `SoloDiffView` chỉ hỗ trợ HEAD ↔ working tree |
| Không có Force Push | Thao tác nguy hiểm, cần thêm bước xác nhận ở host |
| Picker branch nằm trong panel thay vì quick pick ở giữa màn hình | Extension chưa mở được modal |
| Cây vẫn được gửi trọn mỗi lần render | Ảo hóa mới ở phía layout; repository cực lớn vẫn tốn chi phí copy qua WASM |
