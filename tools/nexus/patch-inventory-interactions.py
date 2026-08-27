from pathlib import Path

path = Path("ui/src/nexus/inventory.rs")
text = path.read_text()

old = '''        if element.get_attribute("role").as_deref() != Some("button")
            || element.has_attribute("data-nexus-primary")
            || element.has_attribute("data-nexus-overflow")
        {
            continue;
        }
'''
new = '''        let is_button = element.tag_name() == "BUTTON"
            || element.get_attribute("role").as_deref() == Some("button");
        if !is_button
            || element.has_attribute("data-nexus-primary")
            || element.has_attribute("data-nexus-overflow")
        {
            continue;
        }
'''
if old not in text:
    raise SystemExit("action-classification block not found")
text = text.replace(old, new, 1)

old = '''                <div class="nexus-inventory-table-shell" onclick={on_table_click}>{VNode::from(GuestPanel::new())}</div>
'''
new = '''                <div class="nexus-inventory-table-shell" onmousedown={on_table_click}>{VNode::from(GuestPanel::new())}</div>
'''
if old not in text:
    raise SystemExit("table event binding not found")
text = text.replace(old, new, 1)

path.write_text(text)
