//! Windows UI Automation — accessibility tree introspection.
//!
//! Provides structured access to the Windows UI Automation (UIA) tree
//! for reading UI element properties, finding controls, and building
//! element hierarchies. Uses PowerShell as a bridge to UIA COM APIs.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ── Control Type ────────────────────────────────────────────────────────────

/// Represents the type of a UI Automation control element.
///
/// Maps to the UIA control-type IDs defined by Microsoft's UI Automation
/// specification. Each variant corresponds to a standard Windows control
/// (buttons, text fields, menus, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlType {
    Window,
    Button,
    Edit,
    Text,
    ComboBox,
    ListItem,
    List,
    CheckBox,
    RadioButton,
    Tab,
    TabItem,
    Menu,
    MenuItem,
    Tree,
    TreeItem,
    DataGrid,
    DataItem,
    ScrollBar,
    Slider,
    Hyperlink,
    Image,
    Group,
    Pane,
    ToolBar,
    StatusBar,
    Custom,
    Unknown,
}

impl ControlType {
    /// Convert from a UIA numeric control-type identifier.
    ///
    /// See: <https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-controltype-ids>
    pub fn from_uia_id(id: u32) -> Self {
        match id {
            50032 => Self::Window,
            50000 => Self::Button,
            50004 => Self::Edit,
            50020 => Self::Text,
            50003 => Self::ComboBox,
            50007 => Self::ListItem,
            50008 => Self::List,
            50002 => Self::CheckBox,
            50013 => Self::RadioButton,
            50018 => Self::Tab,
            50019 => Self::TabItem,
            50011 => Self::Menu,
            50012 => Self::MenuItem,
            50023 => Self::Tree,
            50024 => Self::TreeItem,
            50028 => Self::DataGrid,
            50029 => Self::DataItem,
            50014 => Self::ScrollBar,
            50015 => Self::Slider,
            50005 => Self::Hyperlink,
            50006 => Self::Image,
            50026 => Self::Group,
            50033 => Self::Pane,
            50021 => Self::ToolBar,
            50017 => Self::StatusBar,
            50025 => Self::Custom,
            _ => Self::Unknown,
        }
    }

    /// Convert from a human-readable control type name.
    ///
    /// Accepts various aliases (e.g. "textbox" -> Edit, "dropdown" -> ComboBox).
    pub fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "window" => Self::Window,
            "button" => Self::Button,
            "edit" | "textbox" | "input" => Self::Edit,
            "text" | "label" | "static" => Self::Text,
            "combobox" | "combo" | "dropdown" => Self::ComboBox,
            "listitem" | "list item" => Self::ListItem,
            "list" | "listbox" => Self::List,
            "checkbox" | "check box" => Self::CheckBox,
            "radiobutton" | "radio button" => Self::RadioButton,
            "tab" | "tabcontrol" => Self::Tab,
            "tabitem" | "tab item" => Self::TabItem,
            "menu" | "menubar" => Self::Menu,
            "menuitem" | "menu item" => Self::MenuItem,
            "tree" | "treeview" => Self::Tree,
            "treeitem" | "tree item" => Self::TreeItem,
            "datagrid" | "data grid" | "table" => Self::DataGrid,
            "dataitem" | "data item" | "row" => Self::DataItem,
            "scrollbar" | "scroll bar" => Self::ScrollBar,
            "slider" => Self::Slider,
            "hyperlink" | "link" => Self::Hyperlink,
            "image" | "picture" => Self::Image,
            "group" | "groupbox" => Self::Group,
            "pane" | "panel" => Self::Pane,
            "toolbar" | "tool bar" => Self::ToolBar,
            "statusbar" | "status bar" => Self::StatusBar,
            "custom" => Self::Custom,
            _ => Self::Unknown,
        }
    }

    /// Returns `true` if this control type is typically interactive (clickable,
    /// editable, selectable).
    pub fn is_interactive(&self) -> bool {
        matches!(
            self,
            Self::Button
                | Self::Edit
                | Self::ComboBox
                | Self::CheckBox
                | Self::RadioButton
                | Self::Hyperlink
                | Self::Slider
                | Self::MenuItem
                | Self::ListItem
                | Self::TabItem
                | Self::TreeItem
        )
    }

    /// Returns `true` if this control type is a container that holds child
    /// elements.
    pub fn is_container(&self) -> bool {
        matches!(
            self,
            Self::Window
                | Self::List
                | Self::Tree
                | Self::DataGrid
                | Self::Group
                | Self::Pane
                | Self::Tab
                | Self::Menu
                | Self::ToolBar
        )
    }
}

// ── Bounding Rectangle ─────────────────────────────────────────────────────

/// Axis-aligned bounding rectangle for a UI element, in screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundingRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl BoundingRect {
    /// Create a new bounding rectangle.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Return the center point `(cx, cy)` of this rectangle.
    pub fn center(&self) -> (i32, i32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }

    /// Test whether a point `(px, py)` is inside this rectangle.
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }

    /// Return the area in square pixels.
    pub fn area(&self) -> i32 {
        self.width * self.height
    }

    /// Test whether this rectangle overlaps with `other`.
    pub fn intersects(&self, other: &BoundingRect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }
}

// ── UI Element ──────────────────────────────────────────────────────────────

/// A single node in the UI Automation tree.
///
/// Each element carries its own properties (name, type, bounding box, etc.)
/// and a list of direct children, forming a tree structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIElement {
    /// Display name of the element (e.g. "Save", "OK", "File").
    pub name: String,
    /// The UIA control type.
    pub control_type: ControlType,
    /// The AutomationId property (developer-assigned, often stable).
    pub automation_id: String,
    /// Win32 window class name (e.g. "Button", "Edit").
    pub class_name: String,
    /// The current value (text contents for Edit controls, etc.).
    pub value: Option<String>,
    /// Whether the element is enabled for interaction.
    pub is_enabled: bool,
    /// Whether the element is currently visible on screen.
    pub is_visible: bool,
    /// Screen-space bounding rectangle (if available).
    pub bounding_rect: Option<BoundingRect>,
    /// Direct children in the UIA tree.
    pub children: Vec<UIElement>,
    /// Arbitrary extra properties (pattern availability, toggle state, etc.).
    pub properties: HashMap<String, String>,
}

impl UIElement {
    /// Create a minimal element with only a name and control type.
    pub fn new(name: &str, control_type: ControlType) -> Self {
        Self {
            name: name.to_string(),
            control_type,
            automation_id: String::new(),
            class_name: String::new(),
            value: None,
            is_enabled: true,
            is_visible: true,
            bounding_rect: None,
            children: Vec::new(),
            properties: HashMap::new(),
        }
    }

    /// Returns `true` if this element can be interacted with (correct type,
    /// enabled, and visible).
    pub fn is_interactive(&self) -> bool {
        self.control_type.is_interactive() && self.is_enabled && self.is_visible
    }

    /// Number of direct children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Recursively find all elements whose name contains `name`
    /// (case-insensitive).
    pub fn find_by_name(&self, name: &str) -> Vec<&UIElement> {
        let mut results = Vec::new();
        let lower = name.to_lowercase();
        if self.name.to_lowercase().contains(&lower) {
            results.push(self);
        }
        for child in &self.children {
            results.extend(child.find_by_name(name));
        }
        results
    }

    /// Recursively find all elements matching the given control type.
    pub fn find_by_type(&self, ct: &ControlType) -> Vec<&UIElement> {
        let mut results = Vec::new();
        if &self.control_type == ct {
            results.push(self);
        }
        for child in &self.children {
            results.extend(child.find_by_type(ct));
        }
        results
    }

    /// Recursively find the first element with the given automation ID.
    pub fn find_by_automation_id(&self, aid: &str) -> Option<&UIElement> {
        if self.automation_id == aid {
            return Some(self);
        }
        for child in &self.children {
            if let Some(found) = child.find_by_automation_id(aid) {
                return Some(found);
            }
        }
        None
    }

    /// Return only direct children that are interactive.
    pub fn interactive_children(&self) -> Vec<&UIElement> {
        self.children.iter().filter(|c| c.is_interactive()).collect()
    }

    /// Flatten the entire subtree (including self) into a vector via
    /// depth-first traversal.
    pub fn flatten(&self) -> Vec<&UIElement> {
        let mut all = vec![self];
        for child in &self.children {
            all.extend(child.flatten());
        }
        all
    }

    /// Maximum depth of the subtree rooted at this element (0 for leaves).
    pub fn depth(&self) -> usize {
        if self.children.is_empty() {
            0
        } else {
            1 + self.children.iter().map(|c| c.depth()).max().unwrap_or(0)
        }
    }
}

// ── Element Search Criteria ─────────────────────────────────────────────────

/// A composable search filter for finding elements in the UIA tree.
///
/// All populated fields must match (logical AND). Unpopulated fields
/// (None) are treated as "don't care".
#[derive(Debug, Clone, Default)]
pub struct SearchCriteria {
    pub name: Option<String>,
    pub control_type: Option<ControlType>,
    pub automation_id: Option<String>,
    pub class_name: Option<String>,
    pub is_enabled: Option<bool>,
    pub is_visible: Option<bool>,
}

impl SearchCriteria {
    /// Create criteria that match elements by (substring) name.
    pub fn by_name(name: &str) -> Self {
        Self {
            name: Some(name.to_string()),
            ..Default::default()
        }
    }

    /// Create criteria that match elements by control type.
    pub fn by_type(ct: ControlType) -> Self {
        Self {
            control_type: Some(ct),
            ..Default::default()
        }
    }

    /// Create criteria that match elements by automation ID (exact).
    pub fn by_automation_id(aid: &str) -> Self {
        Self {
            automation_id: Some(aid.to_string()),
            ..Default::default()
        }
    }

    /// Test whether `element` satisfies all populated criteria.
    pub fn matches(&self, element: &UIElement) -> bool {
        if let Some(ref n) = self.name {
            if !element.name.to_lowercase().contains(&n.to_lowercase()) {
                return false;
            }
        }
        if let Some(ref ct) = self.control_type {
            if &element.control_type != ct {
                return false;
            }
        }
        if let Some(ref aid) = self.automation_id {
            if element.automation_id != *aid {
                return false;
            }
        }
        if let Some(ref cn) = self.class_name {
            if !element
                .class_name
                .to_lowercase()
                .contains(&cn.to_lowercase())
            {
                return false;
            }
        }
        if let Some(enabled) = self.is_enabled {
            if element.is_enabled != enabled {
                return false;
            }
        }
        if let Some(visible) = self.is_visible {
            if element.is_visible != visible {
                return false;
            }
        }
        true
    }
}

// ── UI Tree Walker ──────────────────────────────────────────────────────────

/// Utility for walking a [`UIElement`] tree with [`SearchCriteria`].
pub struct UITreeWalker;

impl UITreeWalker {
    /// Recursively collect all elements matching `criteria`.
    pub fn search<'a>(root: &'a UIElement, criteria: &SearchCriteria) -> Vec<&'a UIElement> {
        let mut results = Vec::new();
        if criteria.matches(root) {
            results.push(root);
        }
        for child in &root.children {
            results.extend(Self::search(child, criteria));
        }
        results
    }

    /// Return the first element matching `criteria` (depth-first).
    pub fn search_first<'a>(
        root: &'a UIElement,
        criteria: &SearchCriteria,
    ) -> Option<&'a UIElement> {
        if criteria.matches(root) {
            return Some(root);
        }
        for child in &root.children {
            if let Some(found) = Self::search_first(child, criteria) {
                return Some(found);
            }
        }
        None
    }

    /// Count the total number of elements matching `criteria`.
    pub fn count_matches(root: &UIElement, criteria: &SearchCriteria) -> usize {
        let mut count = if criteria.matches(root) { 1 } else { 0 };
        for child in &root.children {
            count += Self::count_matches(child, criteria);
        }
        count
    }

    /// Find the path of element names from `root` to the first element whose
    /// name exactly equals `target_name`.
    ///
    /// Returns `None` if no such element exists in the subtree.
    pub fn element_path(root: &UIElement, target_name: &str) -> Option<Vec<String>> {
        if root.name == target_name {
            return Some(vec![root.name.clone()]);
        }
        for child in &root.children {
            if let Some(mut path) = Self::element_path(child, target_name) {
                path.insert(0, root.name.clone());
                return Some(path);
            }
        }
        None
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper: build a sample UI tree ──────────────────────────────────────

    /// Builds a realistic UI tree for testing:
    ///
    /// ```text
    /// MainWindow (Window)
    ///   +-- MenuBar (Menu)
    ///   |     +-- FileMenu (MenuItem)
    ///   |     +-- EditMenu (MenuItem)
    ///   +-- ContentPane (Pane)
    ///   |     +-- SearchBox (Edit, automation_id="searchInput")
    ///   |     +-- ResultsList (List)
    ///   |           +-- Item1 (ListItem)
    ///   |           +-- Item2 (ListItem, disabled)
    ///   +-- StatusBar (StatusBar)
    ///         +-- StatusText (Text)
    ///         +-- ProgressSlider (Slider)
    /// ```
    fn sample_tree() -> UIElement {
        let mut root = UIElement::new("MainWindow", ControlType::Window);
        root.class_name = "MainWindowClass".to_string();
        root.bounding_rect = Some(BoundingRect::new(0, 0, 1920, 1080));

        // Menu bar
        let mut menu_bar = UIElement::new("MenuBar", ControlType::Menu);
        menu_bar.bounding_rect = Some(BoundingRect::new(0, 0, 1920, 30));

        let file_menu = UIElement::new("FileMenu", ControlType::MenuItem);
        let edit_menu = UIElement::new("EditMenu", ControlType::MenuItem);
        menu_bar.children.push(file_menu);
        menu_bar.children.push(edit_menu);

        // Content pane
        let mut content = UIElement::new("ContentPane", ControlType::Pane);
        content.bounding_rect = Some(BoundingRect::new(0, 30, 1920, 1000));

        let mut search_box = UIElement::new("SearchBox", ControlType::Edit);
        search_box.automation_id = "searchInput".to_string();
        search_box.class_name = "TextBox".to_string();
        search_box.value = Some(String::new());
        search_box.bounding_rect = Some(BoundingRect::new(10, 40, 300, 25));

        let mut results_list = UIElement::new("ResultsList", ControlType::List);
        results_list.bounding_rect = Some(BoundingRect::new(10, 70, 300, 900));

        let item1 = UIElement::new("Item1", ControlType::ListItem);
        let mut item2 = UIElement::new("Item2", ControlType::ListItem);
        item2.is_enabled = false;

        results_list.children.push(item1);
        results_list.children.push(item2);

        content.children.push(search_box);
        content.children.push(results_list);

        // Status bar
        let mut status_bar = UIElement::new("StatusBar", ControlType::StatusBar);
        status_bar.bounding_rect = Some(BoundingRect::new(0, 1050, 1920, 30));

        let status_text = UIElement::new("StatusText", ControlType::Text);
        let slider = UIElement::new("ProgressSlider", ControlType::Slider);

        status_bar.children.push(status_text);
        status_bar.children.push(slider);

        root.children.push(menu_bar);
        root.children.push(content);
        root.children.push(status_bar);

        root
    }

    // ── ControlType::from_uia_id ────────────────────────────────────────────

    #[test]
    fn test_from_uia_id_window() {
        assert_eq!(ControlType::from_uia_id(50032), ControlType::Window);
    }

    #[test]
    fn test_from_uia_id_button() {
        assert_eq!(ControlType::from_uia_id(50000), ControlType::Button);
    }

    #[test]
    fn test_from_uia_id_edit() {
        assert_eq!(ControlType::from_uia_id(50004), ControlType::Edit);
    }

    #[test]
    fn test_from_uia_id_text() {
        assert_eq!(ControlType::from_uia_id(50020), ControlType::Text);
    }

    #[test]
    fn test_from_uia_id_combobox() {
        assert_eq!(ControlType::from_uia_id(50003), ControlType::ComboBox);
    }

    #[test]
    fn test_from_uia_id_checkbox() {
        assert_eq!(ControlType::from_uia_id(50002), ControlType::CheckBox);
    }

    #[test]
    fn test_from_uia_id_menu_item() {
        assert_eq!(ControlType::from_uia_id(50012), ControlType::MenuItem);
    }

    #[test]
    fn test_from_uia_id_tree_item() {
        assert_eq!(ControlType::from_uia_id(50024), ControlType::TreeItem);
    }

    #[test]
    fn test_from_uia_id_datagrid() {
        assert_eq!(ControlType::from_uia_id(50028), ControlType::DataGrid);
    }

    #[test]
    fn test_from_uia_id_hyperlink() {
        assert_eq!(ControlType::from_uia_id(50005), ControlType::Hyperlink);
    }

    #[test]
    fn test_from_uia_id_unknown() {
        assert_eq!(ControlType::from_uia_id(99999), ControlType::Unknown);
    }

    #[test]
    fn test_from_uia_id_zero() {
        assert_eq!(ControlType::from_uia_id(0), ControlType::Unknown);
    }

    // ── ControlType::from_name ──────────────────────────────────────────────

    #[test]
    fn test_from_name_button() {
        assert_eq!(ControlType::from_name("Button"), ControlType::Button);
    }

    #[test]
    fn test_from_name_textbox_alias() {
        assert_eq!(ControlType::from_name("textbox"), ControlType::Edit);
    }

    #[test]
    fn test_from_name_dropdown_alias() {
        assert_eq!(ControlType::from_name("dropdown"), ControlType::ComboBox);
    }

    #[test]
    fn test_from_name_link_alias() {
        assert_eq!(ControlType::from_name("link"), ControlType::Hyperlink);
    }

    #[test]
    fn test_from_name_case_insensitive() {
        assert_eq!(ControlType::from_name("WINDOW"), ControlType::Window);
        assert_eq!(ControlType::from_name("bUtToN"), ControlType::Button);
    }

    #[test]
    fn test_from_name_unknown() {
        assert_eq!(ControlType::from_name("foobar"), ControlType::Unknown);
    }

    #[test]
    fn test_from_name_table_alias() {
        assert_eq!(ControlType::from_name("table"), ControlType::DataGrid);
    }

    #[test]
    fn test_from_name_panel_alias() {
        assert_eq!(ControlType::from_name("panel"), ControlType::Pane);
    }

    // ── ControlType::is_interactive / is_container ──────────────────────────

    #[test]
    fn test_is_interactive() {
        assert!(ControlType::Button.is_interactive());
        assert!(ControlType::Edit.is_interactive());
        assert!(ControlType::Hyperlink.is_interactive());
        assert!(ControlType::Slider.is_interactive());
        assert!(ControlType::MenuItem.is_interactive());
        assert!(ControlType::ListItem.is_interactive());
        assert!(ControlType::TabItem.is_interactive());
        assert!(ControlType::TreeItem.is_interactive());
    }

    #[test]
    fn test_is_not_interactive() {
        assert!(!ControlType::Window.is_interactive());
        assert!(!ControlType::Text.is_interactive());
        assert!(!ControlType::Image.is_interactive());
        assert!(!ControlType::StatusBar.is_interactive());
        assert!(!ControlType::Pane.is_interactive());
        assert!(!ControlType::Unknown.is_interactive());
    }

    #[test]
    fn test_is_container() {
        assert!(ControlType::Window.is_container());
        assert!(ControlType::List.is_container());
        assert!(ControlType::Tree.is_container());
        assert!(ControlType::DataGrid.is_container());
        assert!(ControlType::Group.is_container());
        assert!(ControlType::Pane.is_container());
        assert!(ControlType::Tab.is_container());
        assert!(ControlType::Menu.is_container());
        assert!(ControlType::ToolBar.is_container());
    }

    #[test]
    fn test_is_not_container() {
        assert!(!ControlType::Button.is_container());
        assert!(!ControlType::Edit.is_container());
        assert!(!ControlType::Text.is_container());
        assert!(!ControlType::Hyperlink.is_container());
        assert!(!ControlType::Unknown.is_container());
    }

    // ── BoundingRect ────────────────────────────────────────────────────────

    #[test]
    fn test_bounding_rect_center() {
        let rect = BoundingRect::new(100, 200, 400, 300);
        assert_eq!(rect.center(), (300, 350));
    }

    #[test]
    fn test_bounding_rect_center_origin() {
        let rect = BoundingRect::new(0, 0, 100, 100);
        assert_eq!(rect.center(), (50, 50));
    }

    #[test]
    fn test_bounding_rect_contains_inside() {
        let rect = BoundingRect::new(10, 20, 100, 50);
        assert!(rect.contains(50, 40));
    }

    #[test]
    fn test_bounding_rect_contains_top_left_corner() {
        let rect = BoundingRect::new(10, 20, 100, 50);
        assert!(rect.contains(10, 20));
    }

    #[test]
    fn test_bounding_rect_contains_outside() {
        let rect = BoundingRect::new(10, 20, 100, 50);
        assert!(!rect.contains(5, 20)); // left of rect
        assert!(!rect.contains(10, 15)); // above rect
        assert!(!rect.contains(110, 40)); // right edge (exclusive)
        assert!(!rect.contains(50, 70)); // bottom edge (exclusive)
    }

    #[test]
    fn test_bounding_rect_area() {
        let rect = BoundingRect::new(0, 0, 1920, 1080);
        assert_eq!(rect.area(), 1920 * 1080);
    }

    #[test]
    fn test_bounding_rect_area_small() {
        let rect = BoundingRect::new(5, 5, 10, 20);
        assert_eq!(rect.area(), 200);
    }

    #[test]
    fn test_bounding_rect_intersects_overlap() {
        let a = BoundingRect::new(0, 0, 100, 100);
        let b = BoundingRect::new(50, 50, 100, 100);
        assert!(a.intersects(&b));
        assert!(b.intersects(&a));
    }

    #[test]
    fn test_bounding_rect_intersects_no_overlap() {
        let a = BoundingRect::new(0, 0, 50, 50);
        let b = BoundingRect::new(100, 100, 50, 50);
        assert!(!a.intersects(&b));
        assert!(!b.intersects(&a));
    }

    #[test]
    fn test_bounding_rect_intersects_adjacent() {
        // Touching but not overlapping (edge-to-edge).
        let a = BoundingRect::new(0, 0, 50, 50);
        let b = BoundingRect::new(50, 0, 50, 50);
        assert!(!a.intersects(&b));
    }

    #[test]
    fn test_bounding_rect_intersects_contained() {
        // b is entirely inside a.
        let a = BoundingRect::new(0, 0, 200, 200);
        let b = BoundingRect::new(50, 50, 50, 50);
        assert!(a.intersects(&b));
        assert!(b.intersects(&a));
    }

    // ── UIElement::new ──────────────────────────────────────────────────────

    #[test]
    fn test_ui_element_new_defaults() {
        let elem = UIElement::new("Test", ControlType::Button);
        assert_eq!(elem.name, "Test");
        assert_eq!(elem.control_type, ControlType::Button);
        assert!(elem.automation_id.is_empty());
        assert!(elem.class_name.is_empty());
        assert!(elem.value.is_none());
        assert!(elem.is_enabled);
        assert!(elem.is_visible);
        assert!(elem.bounding_rect.is_none());
        assert!(elem.children.is_empty());
        assert!(elem.properties.is_empty());
    }

    // ── UIElement::is_interactive ───────────────────────────────────────────

    #[test]
    fn test_element_is_interactive_button() {
        let btn = UIElement::new("OK", ControlType::Button);
        assert!(btn.is_interactive());
    }

    #[test]
    fn test_element_is_not_interactive_when_disabled() {
        let mut btn = UIElement::new("OK", ControlType::Button);
        btn.is_enabled = false;
        assert!(!btn.is_interactive());
    }

    #[test]
    fn test_element_is_not_interactive_when_invisible() {
        let mut btn = UIElement::new("OK", ControlType::Button);
        btn.is_visible = false;
        assert!(!btn.is_interactive());
    }

    #[test]
    fn test_element_is_not_interactive_text() {
        let lbl = UIElement::new("Label", ControlType::Text);
        assert!(!lbl.is_interactive());
    }

    // ── UIElement::child_count ──────────────────────────────────────────────

    #[test]
    fn test_child_count() {
        let tree = sample_tree();
        assert_eq!(tree.child_count(), 3); // MenuBar, ContentPane, StatusBar
    }

    // ── UIElement::find_by_name ─────────────────────────────────────────────

    #[test]
    fn test_find_by_name_exact() {
        let tree = sample_tree();
        let found = tree.find_by_name("SearchBox");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "SearchBox");
    }

    #[test]
    fn test_find_by_name_substring() {
        let tree = sample_tree();
        let found = tree.find_by_name("Menu");
        // MenuBar, FileMenu, EditMenu
        assert_eq!(found.len(), 3);
    }

    #[test]
    fn test_find_by_name_case_insensitive() {
        let tree = sample_tree();
        let found = tree.find_by_name("searchbox");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn test_find_by_name_not_found() {
        let tree = sample_tree();
        let found = tree.find_by_name("NonExistent");
        assert!(found.is_empty());
    }

    // ── UIElement::find_by_type ─────────────────────────────────────────────

    #[test]
    fn test_find_by_type_list_item() {
        let tree = sample_tree();
        let found = tree.find_by_type(&ControlType::ListItem);
        assert_eq!(found.len(), 2); // Item1, Item2
    }

    #[test]
    fn test_find_by_type_window() {
        let tree = sample_tree();
        let found = tree.find_by_type(&ControlType::Window);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "MainWindow");
    }

    #[test]
    fn test_find_by_type_none() {
        let tree = sample_tree();
        let found = tree.find_by_type(&ControlType::CheckBox);
        assert!(found.is_empty());
    }

    // ── UIElement::find_by_automation_id ─────────────────────────────────────

    #[test]
    fn test_find_by_automation_id_found() {
        let tree = sample_tree();
        let found = tree.find_by_automation_id("searchInput");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "SearchBox");
    }

    #[test]
    fn test_find_by_automation_id_not_found() {
        let tree = sample_tree();
        let found = tree.find_by_automation_id("doesNotExist");
        assert!(found.is_none());
    }

    // ── UIElement::interactive_children ──────────────────────────────────────

    #[test]
    fn test_interactive_children() {
        let tree = sample_tree();
        // Root children: MenuBar (Menu, container), ContentPane (Pane, container),
        // StatusBar (StatusBar, not interactive). None are interactive controls.
        let interactive = tree.interactive_children();
        assert!(interactive.is_empty());
    }

    #[test]
    fn test_interactive_children_of_list() {
        let tree = sample_tree();
        // ContentPane -> ResultsList has Item1 (enabled) and Item2 (disabled)
        let results_list = &tree.children[1].children[1]; // ContentPane -> ResultsList
        let interactive = results_list.interactive_children();
        assert_eq!(interactive.len(), 1); // Only Item1 (Item2 is disabled)
        assert_eq!(interactive[0].name, "Item1");
    }

    // ── UIElement::flatten ──────────────────────────────────────────────────

    #[test]
    fn test_flatten_counts_all_nodes() {
        let tree = sample_tree();
        let flat = tree.flatten();
        // MainWindow(1) + MenuBar(1) + FileMenu(1) + EditMenu(1)
        // + ContentPane(1) + SearchBox(1) + ResultsList(1) + Item1(1) + Item2(1)
        // + StatusBar(1) + StatusText(1) + ProgressSlider(1) = 12
        assert_eq!(flat.len(), 12);
    }

    #[test]
    fn test_flatten_leaf() {
        let leaf = UIElement::new("Leaf", ControlType::Text);
        let flat = leaf.flatten();
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].name, "Leaf");
    }

    // ── UIElement::depth ────────────────────────────────────────────────────

    #[test]
    fn test_depth_leaf() {
        let leaf = UIElement::new("Leaf", ControlType::Text);
        assert_eq!(leaf.depth(), 0);
    }

    #[test]
    fn test_depth_tree() {
        let tree = sample_tree();
        // MainWindow -> ContentPane -> ResultsList -> Item1 = depth 3
        assert_eq!(tree.depth(), 3);
    }

    // ── SearchCriteria ──────────────────────────────────────────────────────

    #[test]
    fn test_search_criteria_by_name() {
        let criteria = SearchCriteria::by_name("Save");
        let mut elem = UIElement::new("Save As", ControlType::Button);
        elem.is_enabled = true;
        assert!(criteria.matches(&elem));
    }

    #[test]
    fn test_search_criteria_by_name_no_match() {
        let criteria = SearchCriteria::by_name("Save");
        let elem = UIElement::new("Cancel", ControlType::Button);
        assert!(!criteria.matches(&elem));
    }

    #[test]
    fn test_search_criteria_by_type() {
        let criteria = SearchCriteria::by_type(ControlType::Edit);
        let elem = UIElement::new("Input", ControlType::Edit);
        assert!(criteria.matches(&elem));
    }

    #[test]
    fn test_search_criteria_by_type_mismatch() {
        let criteria = SearchCriteria::by_type(ControlType::Edit);
        let elem = UIElement::new("Input", ControlType::Button);
        assert!(!criteria.matches(&elem));
    }

    #[test]
    fn test_search_criteria_by_automation_id() {
        let criteria = SearchCriteria::by_automation_id("btnOk");
        let mut elem = UIElement::new("OK", ControlType::Button);
        elem.automation_id = "btnOk".to_string();
        assert!(criteria.matches(&elem));
    }

    #[test]
    fn test_search_criteria_combined() {
        let criteria = SearchCriteria {
            name: Some("Save".to_string()),
            control_type: Some(ControlType::Button),
            is_enabled: Some(true),
            ..Default::default()
        };
        let elem = UIElement::new("Save", ControlType::Button);
        assert!(criteria.matches(&elem));

        // Wrong type -> should not match.
        let elem2 = UIElement::new("Save", ControlType::MenuItem);
        assert!(!criteria.matches(&elem2));
    }

    #[test]
    fn test_search_criteria_class_name() {
        let criteria = SearchCriteria {
            class_name: Some("TextBox".to_string()),
            ..Default::default()
        };
        let mut elem = UIElement::new("Input", ControlType::Edit);
        elem.class_name = "TextBox".to_string();
        assert!(criteria.matches(&elem));

        let mut elem2 = UIElement::new("Input", ControlType::Edit);
        elem2.class_name = "ComboBox".to_string();
        assert!(!criteria.matches(&elem2));
    }

    #[test]
    fn test_search_criteria_disabled_filter() {
        let criteria = SearchCriteria {
            is_enabled: Some(false),
            ..Default::default()
        };
        let mut elem = UIElement::new("Btn", ControlType::Button);
        elem.is_enabled = false;
        assert!(criteria.matches(&elem));

        let elem2 = UIElement::new("Btn", ControlType::Button);
        assert!(!criteria.matches(&elem2)); // enabled = true by default
    }

    #[test]
    fn test_search_criteria_visibility_filter() {
        let criteria = SearchCriteria {
            is_visible: Some(false),
            ..Default::default()
        };
        let mut elem = UIElement::new("Hidden", ControlType::Pane);
        elem.is_visible = false;
        assert!(criteria.matches(&elem));

        let elem2 = UIElement::new("Visible", ControlType::Pane);
        assert!(!criteria.matches(&elem2)); // visible = true by default
    }

    #[test]
    fn test_search_criteria_default_matches_everything() {
        let criteria = SearchCriteria::default();
        let elem = UIElement::new("Anything", ControlType::Custom);
        assert!(criteria.matches(&elem));
    }

    // ── UITreeWalker ────────────────────────────────────────────────────────

    #[test]
    fn test_tree_walker_search_all_buttons() {
        let mut root = UIElement::new("Root", ControlType::Window);
        root.children.push(UIElement::new("Btn1", ControlType::Button));
        root.children.push(UIElement::new("Lbl1", ControlType::Text));
        root.children.push(UIElement::new("Btn2", ControlType::Button));

        let criteria = SearchCriteria::by_type(ControlType::Button);
        let found = UITreeWalker::search(&root, &criteria);
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn test_tree_walker_search_in_sample_tree() {
        let tree = sample_tree();
        let criteria = SearchCriteria::by_type(ControlType::MenuItem);
        let found = UITreeWalker::search(&tree, &criteria);
        assert_eq!(found.len(), 2); // FileMenu, EditMenu
    }

    #[test]
    fn test_tree_walker_search_first_found() {
        let tree = sample_tree();
        let criteria = SearchCriteria::by_type(ControlType::ListItem);
        let found = UITreeWalker::search_first(&tree, &criteria);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Item1");
    }

    #[test]
    fn test_tree_walker_search_first_not_found() {
        let tree = sample_tree();
        let criteria = SearchCriteria::by_type(ControlType::CheckBox);
        let found = UITreeWalker::search_first(&tree, &criteria);
        assert!(found.is_none());
    }

    #[test]
    fn test_tree_walker_count_matches() {
        let tree = sample_tree();
        let criteria = SearchCriteria::by_type(ControlType::ListItem);
        assert_eq!(UITreeWalker::count_matches(&tree, &criteria), 2);
    }

    #[test]
    fn test_tree_walker_count_matches_zero() {
        let tree = sample_tree();
        let criteria = SearchCriteria::by_type(ControlType::RadioButton);
        assert_eq!(UITreeWalker::count_matches(&tree, &criteria), 0);
    }

    #[test]
    fn test_tree_walker_element_path_found() {
        let tree = sample_tree();
        let path = UITreeWalker::element_path(&tree, "Item1");
        assert!(path.is_some());
        let path = path.unwrap();
        assert_eq!(
            path,
            vec!["MainWindow", "ContentPane", "ResultsList", "Item1"]
        );
    }

    #[test]
    fn test_tree_walker_element_path_root() {
        let tree = sample_tree();
        let path = UITreeWalker::element_path(&tree, "MainWindow");
        assert_eq!(path, Some(vec!["MainWindow".to_string()]));
    }

    #[test]
    fn test_tree_walker_element_path_not_found() {
        let tree = sample_tree();
        let path = UITreeWalker::element_path(&tree, "Nonexistent");
        assert!(path.is_none());
    }

    // ── Serialization round-trip ────────────────────────────────────────────

    #[test]
    fn test_serialize_deserialize_control_type() {
        let ct = ControlType::Button;
        let json = serde_json::to_string(&ct).unwrap();
        let ct2: ControlType = serde_json::from_str(&json).unwrap();
        assert_eq!(ct, ct2);
    }

    #[test]
    fn test_serialize_deserialize_bounding_rect() {
        let rect = BoundingRect::new(10, 20, 300, 150);
        let json = serde_json::to_string(&rect).unwrap();
        let rect2: BoundingRect = serde_json::from_str(&json).unwrap();
        assert_eq!(rect, rect2);
    }

    #[test]
    fn test_serialize_deserialize_ui_element() {
        let mut elem = UIElement::new("TestBtn", ControlType::Button);
        elem.automation_id = "btn1".to_string();
        elem.class_name = "Button".to_string();
        elem.value = Some("Click me".to_string());
        elem.bounding_rect = Some(BoundingRect::new(100, 200, 80, 30));
        elem.properties
            .insert("IsInvokePatternAvailable".to_string(), "true".to_string());

        let json = serde_json::to_string(&elem).unwrap();
        let elem2: UIElement = serde_json::from_str(&json).unwrap();

        assert_eq!(elem2.name, "TestBtn");
        assert_eq!(elem2.control_type, ControlType::Button);
        assert_eq!(elem2.automation_id, "btn1");
        assert_eq!(elem2.value, Some("Click me".to_string()));
        assert_eq!(elem2.bounding_rect, Some(BoundingRect::new(100, 200, 80, 30)));
        assert_eq!(
            elem2.properties.get("IsInvokePatternAvailable"),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn test_serialize_deserialize_tree() {
        let tree = sample_tree();
        let json = serde_json::to_string(&tree).unwrap();
        let tree2: UIElement = serde_json::from_str(&json).unwrap();

        assert_eq!(tree2.name, "MainWindow");
        assert_eq!(tree2.child_count(), 3);
        assert_eq!(tree2.flatten().len(), 12);
    }

    // ── Edge cases ──────────────────────────────────────────────────────────

    #[test]
    fn test_empty_element_flatten() {
        let elem = UIElement::new("", ControlType::Unknown);
        assert_eq!(elem.flatten().len(), 1);
        assert_eq!(elem.depth(), 0);
        assert_eq!(elem.child_count(), 0);
    }

    #[test]
    fn test_find_by_name_empty_string() {
        let tree = sample_tree();
        // Empty string is contained in every name.
        let found = tree.find_by_name("");
        assert_eq!(found.len(), 12);
    }

    #[test]
    fn test_bounding_rect_zero_size() {
        let rect = BoundingRect::new(50, 50, 0, 0);
        assert_eq!(rect.area(), 0);
        assert_eq!(rect.center(), (50, 50));
        assert!(!rect.contains(50, 50));
    }
}
