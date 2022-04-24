use crate::tools::asset_info::Search;
use crate::ui::widgets::logged_in::refresh::Refresh;
use asset::EpicAsset;
use glib::clone;
use gtk4::{self, gdk_pixbuf, prelude::*, CustomSorter};
use gtk4::{gio, glib, subclass::prelude::*, CompositeTemplate};
use gtk_macros::action;
use log::debug;
use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};

mod add_to_project;
mod asset;
mod asset_actions;
pub mod asset_detail;
mod create_asset_project;
mod download_detail;
pub mod image_stack;
mod sidebar;

pub(crate) mod imp {
    use super::*;
    use crate::config;
    use crate::ui::widgets::download_manager::EpicDownloadManager;
    use crate::window::EpicAssetManagerWindow;
    use gtk4::gio;
    use gtk4::gio::ListStore;
    use gtk4::glib::{Object, ParamSpec, ParamSpecBoolean, ParamSpecString, ParamSpecUInt};
    use once_cell::sync::OnceCell;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;
    use threadpool::ThreadPool;

    #[derive(Debug, CompositeTemplate)]
    #[template(resource = "/io/github/achetagames/epic_asset_manager/library.ui")]
    pub struct EpicLibraryBox {
        #[template_child]
        pub details:
            TemplateChild<crate::ui::widgets::logged_in::library::asset_detail::EpicAssetDetails>,
        #[template_child]
        pub sidebar: TemplateChild<sidebar::EpicSidebar>,
        #[template_child]
        pub asset_grid: TemplateChild<gtk4::GridView>,
        #[template_child]
        pub asset_search: TemplateChild<gtk4::SearchEntry>,
        #[template_child]
        pub search_toggle: TemplateChild<gtk4::ToggleButton>,
        #[template_child]
        pub select_order_by: TemplateChild<gtk4::ComboBoxText>,
        #[template_child]
        pub order: TemplateChild<gtk4::Button>,
        #[template_child]
        pub count_label: TemplateChild<gtk4::Label>,
        #[template_child]
        pub refresh_progress: TemplateChild<gtk4::ProgressBar>,
        pub sidebar_expanded: RefCell<bool>,
        pub filter: RefCell<Option<String>>,
        pub search: RefCell<Option<String>>,
        pub actions: gio::SimpleActionGroup,
        pub window: OnceCell<EpicAssetManagerWindow>,
        pub download_manager: OnceCell<EpicDownloadManager>,
        pub filter_model: gtk4::FilterListModel,
        pub sorter_model: gtk4::SortListModel,
        pub grid_model: ListStore,
        pub loaded_assets: RefCell<HashMap<String, egs_api::api::types::asset_info::AssetInfo>>,
        pub loaded_data: RefCell<HashMap<String, crate::models::asset_data::AssetData>>,
        pub asset_product_names: RefCell<HashMap<String, String>>,
        pub asset_load_pool: ThreadPool,
        pub image_load_pool: ThreadPool,
        pub assets_pending: Arc<std::sync::RwLock<Vec<Object>>>,
        pub categories: RefCell<HashSet<String>>,
        pub settings: gio::Settings,
        loading: RefCell<u32>,
        loaded: RefCell<u32>,
        item: RefCell<Option<String>>,
        product: RefCell<Option<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EpicLibraryBox {
        const NAME: &'static str = "EpicLibraryBox";
        type Type = super::EpicLibraryBox;
        type ParentType = gtk4::Box;

        fn new() -> Self {
            Self {
                details: TemplateChild::default(),
                sidebar: TemplateChild::default(),
                asset_grid: TemplateChild::default(),
                asset_search: TemplateChild::default(),
                search_toggle: TemplateChild::default(),
                select_order_by: TemplateChild::default(),
                order: TemplateChild::default(),
                count_label: TemplateChild::default(),
                refresh_progress: TemplateChild::default(),
                sidebar_expanded: RefCell::new(false),
                filter: RefCell::new(None),
                search: RefCell::new(None),
                actions: gio::SimpleActionGroup::new(),
                window: OnceCell::new(),
                download_manager: OnceCell::new(),
                filter_model: gtk4::FilterListModel::new(gio::ListModel::NONE, gtk4::Filter::NONE),
                sorter_model: gtk4::SortListModel::new(gio::ListModel::NONE, gtk4::Sorter::NONE),
                grid_model: gio::ListStore::new(crate::models::asset_data::AssetData::static_type()),
                loaded_assets: RefCell::new(HashMap::new()),
                loaded_data: RefCell::new(HashMap::new()),
                asset_product_names: RefCell::new(HashMap::new()),
                asset_load_pool: ThreadPool::with_name("Asset Load Pool".to_string(), 5),
                image_load_pool: ThreadPool::with_name("Image Load Pool".to_string(), 5),
                assets_pending: Arc::new(std::sync::RwLock::new(vec![])),
                categories: RefCell::new(HashSet::new()),
                settings: gio::Settings::new(config::APP_ID),
                loading: RefCell::new(0),
                loaded: RefCell::new(0),
                item: RefCell::new(None),
                product: RefCell::new(None),
            }
        }

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
        }

        // You must call `Widget`'s `init_template()` within `instance_init()`.
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for EpicLibraryBox {
        fn properties() -> &'static [ParamSpec] {
            use once_cell::sync::Lazy;

            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![
                    ParamSpecBoolean::new(
                        "sidebar-expanded",
                        "sidebar expanded",
                        "Is Sidebar expanded",
                        false,
                        glib::ParamFlags::READWRITE,
                    ),
                    ParamSpecUInt::new(
                        "to-load",
                        "to load",
                        "Assets to load",
                        0,
                        u32::MAX,
                        0,
                        glib::ParamFlags::READWRITE,
                    ),
                    ParamSpecUInt::new(
                        "loaded",
                        "loaded",
                        "Assets to load",
                        0,
                        u32::MAX,
                        0,
                        glib::ParamFlags::READWRITE,
                    ),
                    ParamSpecString::new(
                        "filter",
                        "Filter",
                        "Filter",
                        None,
                        glib::ParamFlags::READWRITE,
                    ),
                    ParamSpecString::new(
                        "search",
                        "Search",
                        "Search",
                        None,
                        glib::ParamFlags::READWRITE,
                    ),
                    ParamSpecString::new("item", "item", "item", None, glib::ParamFlags::READWRITE),
                    ParamSpecString::new(
                        "product",
                        "product",
                        "product",
                        None,
                        glib::ParamFlags::READWRITE,
                    ),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn set_property(
            &self,
            obj: &Self::Type,
            _id: usize,
            value: &glib::Value,
            pspec: &ParamSpec,
        ) {
            match pspec.name() {
                "sidebar-expanded" => {
                    let sidebar_expanded = value.get().unwrap();
                    self.sidebar_expanded.replace(sidebar_expanded);
                }
                "to-load" => {
                    self.loading.replace(value.get().unwrap());
                }
                "loaded" => {
                    self.loaded.replace(value.get().unwrap());
                }
                "filter" => {
                    let filter: Option<String> = value.get().unwrap();

                    self.filter.replace(match filter {
                        None => None,
                        Some(f) => {
                            if f.is_empty() {
                                None
                            } else {
                                Some(f)
                            }
                        }
                    });
                    obj.apply_filter();
                }
                "search" => {
                    let search: Option<String> = value.get().unwrap();
                    self.search.replace(match search {
                        None => None,
                        Some(f) => {
                            if f.is_empty() {
                                None
                            } else {
                                Some(f)
                            }
                        }
                    });
                    obj.apply_filter();
                }
                "item" => {
                    let item = value.get().unwrap();
                    self.product.replace(None);
                    self.item.replace(item);
                    obj.open_asset();
                }
                "product" => {
                    let product = value.get().unwrap();
                    self.item.replace(None);
                    self.product.replace(product);
                    obj.open_asset();
                }
                _ => unimplemented!(),
            }
        }

        fn property(&self, _obj: &Self::Type, _id: usize, pspec: &ParamSpec) -> glib::Value {
            match pspec.name() {
                "sidebar-expanded" => self.sidebar_expanded.borrow().to_value(),
                "to-load" => self.loading.borrow().to_value(),
                "loaded" => self.loaded.borrow().to_value(),
                "filter" => self.filter.borrow().to_value(),
                "search" => self.search.borrow().to_value(),
                "item" => self.item.borrow().to_value(),
                "product" => self.product.borrow().to_value(),
                _ => unimplemented!(),
            }
        }

        fn constructed(&self, obj: &Self::Type) {
            self.parent_constructed(obj);
            obj.bind_properties();
            obj.setup_actions();
            obj.setup_widgets();
        }
    }

    impl WidgetImpl for EpicLibraryBox {}
    impl BoxImpl for EpicLibraryBox {}
}

glib::wrapper! {
    pub struct EpicLibraryBox(ObjectSubclass<imp::EpicLibraryBox>)
        @extends gtk4::Widget, gtk4::Box;
}

impl Default for EpicLibraryBox {
    fn default() -> Self {
        Self::new()
    }
}

impl EpicLibraryBox {
    pub fn new() -> Self {
        glib::Object::new(&[]).expect("Failed to create EpicLibraryBox")
    }

    pub fn set_download_manager(
        &self,
        dm: &crate::ui::widgets::download_manager::EpicDownloadManager,
    ) {
        let self_ = self.imp();
        // Do not run this twice
        if self_.download_manager.get().is_some() {
            return;
        }

        self_.download_manager.set(dm.clone()).unwrap();
        self_.details.set_download_manager(dm);
    }

    pub fn set_window(&self, window: &crate::window::EpicAssetManagerWindow) {
        let self_ = self.imp();
        // Do not run this twice
        if self_.window.get().is_some() {
            return;
        }

        self_.window.set(window.clone()).unwrap();
        self_.details.set_window(&window.clone());
        self_
            .asset_search
            .set_key_capture_widget(Some(&window.clone()));
        let factory = gtk4::SignalListItemFactory::new();
        // Create the children
        factory.connect_setup(move |_factory, item| {
            let row = EpicAsset::new();
            item.set_child(Some(&row));
        });

        // Populate children
        factory.connect_bind(move |_, list_item| {
            Self::populate_model(list_item);
        });

        self_.filter_model.set_model(Some(&self_.grid_model));
        self.update_count();
        self_.sorter_model.set_model(Some(&self_.filter_model));
        self_
            .sorter_model
            .set_sorter(Some(&Self::sorter("name", true)));
        let selection_model = gtk4::SingleSelection::new(Some(&self_.sorter_model));
        selection_model.set_autoselect(false);
        selection_model.set_can_unselect(true);
        self_.asset_grid.set_model(Some(&selection_model));
        self_.asset_grid.set_factory(Some(&factory));

        selection_model.connect_selected_notify(clone!(@weak self as library => move |model| {
            library.asset_selected(model);
        }));

        self.fetch_assets();
    }

    fn asset_selected(&self, model: &gtk4::SingleSelection) {
        if let Some(a) = model.selected_item() {
            let self_ = self.imp();
            let asset = a
                .downcast::<crate::models::asset_data::AssetData>()
                .unwrap();
            let assets = self_.loaded_assets.borrow();
            if let Some(a) = assets.get(&asset.id()) {
                self_.details.set_asset(a);
            }
            self_.details.set_property("position", model.selected());
        }
    }

    fn populate_model(list_item: &gtk4::ListItem) {
        let data = list_item
            .item()
            .unwrap()
            .downcast::<crate::models::asset_data::AssetData>()
            .unwrap();

        let child = list_item.child().unwrap().downcast::<EpicAsset>().unwrap();
        child.set_data(&data);
    }

    fn sorter(by: &str, asc: bool) -> CustomSorter {
        match by {
            "released" => gtk4::CustomSorter::new(move |obj1, obj2| {
                let info1 = obj1
                    .downcast_ref::<crate::models::asset_data::AssetData>()
                    .unwrap()
                    .release();
                let info2 = obj2
                    .downcast_ref::<crate::models::asset_data::AssetData>()
                    .unwrap()
                    .release();
                if info1.is_none() {
                    return gtk4::Ordering::Smaller;
                } else if info2.is_none() {
                    return gtk4::Ordering::Larger;
                }
                if asc {
                    info1.unwrap().cmp(&info2.unwrap()).into()
                } else {
                    info2.unwrap().cmp(&info1.unwrap()).into()
                }
            }),
            "updated" => gtk4::CustomSorter::new(move |obj1, obj2| {
                let info1 = obj1
                    .downcast_ref::<crate::models::asset_data::AssetData>()
                    .unwrap()
                    .last_modified();
                let info2 = obj2
                    .downcast_ref::<crate::models::asset_data::AssetData>()
                    .unwrap()
                    .last_modified();
                if info1.is_none() {
                    return gtk4::Ordering::Smaller;
                } else if info2.is_none() {
                    return gtk4::Ordering::Larger;
                }
                if asc {
                    info1.unwrap().cmp(&info2.unwrap()).into()
                } else {
                    info2.unwrap().cmp(&info1.unwrap()).into()
                }
            }),
            _ => gtk4::CustomSorter::new(move |obj1, obj2| {
                let info1 = obj1
                    .downcast_ref::<crate::models::asset_data::AssetData>()
                    .unwrap();
                let info2 = obj2
                    .downcast_ref::<crate::models::asset_data::AssetData>()
                    .unwrap();
                if asc {
                    info1
                        .name()
                        .to_lowercase()
                        .cmp(&info2.name().to_lowercase())
                        .into()
                } else {
                    info2
                        .name()
                        .to_lowercase()
                        .cmp(&info1.name().to_lowercase())
                        .into()
                }
            }),
        }
    }

    /// Open asset based on a name from xdg-open
    fn open_asset(&self) {
        let self_ = self.imp();
        if let Some(id) = self.item() {
            let assets = self_.loaded_assets.borrow();
            if let Some(a) = assets.get(&id) {
                self_.details.set_asset(a);
            }
        } else if let Some(product) = self.product() {
            let assets = self_.loaded_assets.borrow();
            let products = self_.asset_product_names.borrow();
            match products.get(&product) {
                Some(id) => {
                    if let Some(a) = assets.get(id) {
                        self_.details.set_asset(a);
                    }
                }
                None => {
                    for prod in products.keys() {
                        if product.starts_with(prod) {
                            if let Some(id) = products.get(prod) {
                                if let Some(a) = assets.get(id) {
                                    self_.details.set_asset(a);
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    pub fn flush_assets(&self) {
        // let start = std::time::Instant::now();
        let self_ = self.imp();
        if let Ok(mut vec) = self_.assets_pending.write() {
            if vec.is_empty() {
                return;
            }
            self_.grid_model.splice(0, 0, vec.as_slice());
            vec.clear();
        }
        self.update_count();
        // Scroll to top if nothing is selected
        if !self_.details.has_asset() {
            match self_.asset_grid.vadjustment() {
                None => {}
                Some(adj) => adj.set_value(0.0),
            };
        }
        if self.can_be_refreshed() {
            self.refresh_state_changed();
        }
        self.open_asset();
        // debug!("Finished flushing {:?}", start.elapsed());
    }

    pub fn update_count(&self) {
        let self_ = self.imp();
        let count = self_.filter_model.n_items();
        self_.count_label.set_label(&format!(
            "{} {}",
            count,
            if count == 1 { "item" } else { "items" }
        ));
    }

    pub fn bind_properties(&self) {
        let self_ = self.imp();
        self_
            .asset_search
            .bind_property("text", self, "search")
            .flags(glib::BindingFlags::DEFAULT | glib::BindingFlags::SYNC_CREATE)
            .build();
    }

    pub fn setup_widgets(&self) {
        let self_ = self.imp();

        self_.sidebar.set_logged_in(self);

        self_
            .select_order_by
            .connect_changed(clone!(@weak self as library => move |_| {
                library.order_changed();
            }));

        self_
            .asset_search
            .connect_search_changed(clone!(@weak self as library => move |_| {
                let self_ = library.imp();
                self_.search_toggle.set_active(true);
            }));
    }

    pub fn order_changed(&self) {
        let self_ = self.imp();
        let asc = if let Some(name) = self_.order.icon_name() {
            matches!(name.as_str(), "view-sort-ascending-symbolic")
        } else {
            false
        };
        if let Some(by) = self_.select_order_by.active_id() {
            self_.sorter_model.set_sorter(Some(&Self::sorter(&by, asc)));
        };
    }

    pub fn setup_actions(&self) {
        let self_ = self.imp();

        action!(
            self_.actions,
            "show_download_details",
            clone!(@weak self as library => move |_, _| {
                library.show_download_details();

            })
        );

        action!(
            self_.actions,
            "order",
            clone!(@weak self as library => move |_, _| {
                library.order();
            })
        );

        self.insert_action_group("library", Some(&self_.actions));
    }

    fn show_download_details(&self) {
        let self_ = self.imp();
        if let Some(w) = self_.window.get() {
            w.show_download_manager();
        }
    }

    fn order(&self) {
        let self_ = self.imp();
        if let Some(name) = self_.order.icon_name() {
            match name.as_str() {
                "view-sort-ascending-symbolic" => {
                    self_.order.set_icon_name("view-sort-descending-symbolic");
                }
                _ => self_.order.set_icon_name("view-sort-ascending-symbolic"),
            }
        };
        self.order_changed();
    }

    pub fn filter(&self) -> Option<String> {
        self.property("filter")
    }

    pub fn loading(&self) -> u32 {
        self.property("to-load")
    }

    pub fn loaded(&self) -> u32 {
        self.property("loaded")
    }

    pub fn search(&self) -> Option<String> {
        self.property("search")
    }

    pub fn item(&self) -> Option<String> {
        self.property("item")
    }

    pub fn product(&self) -> Option<String> {
        self.property("product")
    }

    pub fn apply_filter(&self) {
        let self_ = self.imp();
        let search = self.search();
        let filter_p = self.filter();
        if filter_p.is_none() && search.is_none() {
            self_.filter_model.set_filter(None::<&gtk4::CustomFilter>);
            self.update_count();
            return;
        }

        let filter = gtk4::CustomFilter::new(move |object| {
            let asset = object
                .downcast_ref::<crate::models::asset_data::AssetData>()
                .unwrap();
            (match &search {
                None => true,
                Some(se) => asset
                    .name()
                    .to_ascii_lowercase()
                    .contains(&se.to_ascii_lowercase()),
            }) && (match &filter_p {
                None => true,
                Some(f) => asset.check_category(f),
            })
        });
        self_.filter_model.set_filter(Some(&filter));
        self.update_count();
    }

    pub fn add_asset(&self, asset: &egs_api::api::types::asset_info::AssetInfo, image: &[u8]) {
        let self_ = self.imp();
        if let Some(categories) = &asset.categories {
            for category in categories {
                let mut cats = self_.categories.borrow_mut();
                if cats.insert(category.path.clone()) {
                    self_.sidebar.add_category(&category.path);
                }
            }
        };
        if let Some(window) = self.main_window() {
            let win_ = window.imp();
            let sender = win_.model.borrow().sender.clone();
            sender
                .send(crate::ui::messages::Msg::EndAssetProcessing)
                .unwrap();
            let mut assets = self_.loaded_assets.borrow_mut();
            let mut asset_products = self_.asset_product_names.borrow_mut();
            if match assets.get_mut(&asset.id) {
                None => {
                    assets.insert(asset.id.clone(), asset.clone());
                    if let Some(title) = asset.title.clone() {
                        let title: String = title
                            .chars()
                            .filter(|c| c.is_ascii_alphanumeric() || c.is_ascii_whitespace())
                            .collect();
                        let title: String = title.to_lowercase().replace(' ', "-");
                        asset_products.insert(title, asset.id.clone());
                    }
                    true
                }
                Some(a) => {
                    if asset.id.eq(&a.id) {
                        // TODO: update asset if there are changes
                        trace!("Duplicate asset: {}", asset.id);
                        false
                    } else {
                        assets.insert(asset.id.clone(), asset.clone());
                        if let Some(title) = asset.title.clone() {
                            let title: String = title
                                .chars()
                                .filter(|c| c.is_ascii_alphanumeric() || c.is_ascii_whitespace())
                                .collect();
                            let title: String = title.to_lowercase().replace(' ', "-");
                            asset_products.insert(title, asset.id.clone());
                        }
                        true
                    }
                }
            } {
                let data = crate::models::asset_data::AssetData::new(asset, image);
                let mut data_hash = self_.loaded_data.borrow_mut();
                data_hash.insert(data.id(), data.clone());
                if let Ok(mut vec) = self_.assets_pending.write() {
                    vec.push(data.upcast());
                }
            }
        }
    }

    pub fn start_processing_asset(&self) {
        self.set_property("to-load", self.loading() + 1);
        self.update_progress();
    }

    pub fn end_processing_asset(&self) {
        self.set_property("loaded", self.loaded() + 1);
        self.update_progress();
    }

    fn update_progress(&self) {
        let self_ = self.imp();
        self_
            .refresh_progress
            .set_fraction(f64::from(self.loaded()) / f64::from(self.loading()));
        self_
            .refresh_progress
            .set_visible(self.loaded() != self.loading());
    }

    pub fn load_thumbnail(&self, asset: &egs_api::api::types::asset_info::AssetInfo) {
        let self_ = self.imp();
        if let Some(window) = self.main_window() {
            let win_ = window.imp();
            let sender = win_.model.borrow().sender.clone();
            match asset.thumbnail() {
                None => {
                    sender
                        .send(crate::ui::messages::Msg::ProcessAssetThumbnail(
                            asset.clone(),
                            vec![],
                        ))
                        .unwrap();
                }
                Some(t) => {
                    let cache_dir = self_.settings.string("cache-directory").to_string();
                    let mut cache_path = PathBuf::from(cache_dir);
                    cache_path.push("images");
                    let name = Path::new(t.url.path()).extension().and_then(OsStr::to_str);
                    cache_path.push(format!("{}.{}", t.md5, name.unwrap_or(".png")));
                    let asset = asset.clone();
                    self_.image_load_pool.execute(move || {
                        if let Ok(w) = crate::RUNNING.read() {
                            if !*w {
                                return;
                            }
                        }
                        match File::open(cache_path.as_path()) {
                            Ok(mut f) => {
                                fs::create_dir_all(&cache_path.parent().unwrap()).unwrap();
                                let metadata = fs::metadata(&cache_path.as_path())
                                    .expect("unable to read metadata");
                                let mut buffer = vec![0; metadata.len() as usize];
                                f.read_exact(&mut buffer).expect("buffer overflow");
                                let pixbuf_loader = gdk_pixbuf::PixbufLoader::new();
                                pixbuf_loader.write(&buffer).unwrap();
                                pixbuf_loader.close().ok();
                                match pixbuf_loader.pixbuf() {
                                    None => {}
                                    Some(pb) => {
                                        let width = pb.width();
                                        let height = pb.height();

                                        let width_percent = 128.0 / width as f64;
                                        let height_percent = 128.0 / height as f64;
                                        let percent = if height_percent < width_percent {
                                            height_percent
                                        } else {
                                            width_percent
                                        };
                                        let desired =
                                            (width as f64 * percent, height as f64 * percent);
                                        sender
                                            .send(crate::ui::messages::Msg::ProcessAssetThumbnail(
                                                asset.clone(),
                                                pb.scale_simple(
                                                    desired.0.round() as i32,
                                                    desired.1.round() as i32,
                                                    gdk_pixbuf::InterpType::Bilinear,
                                                )
                                                .unwrap()
                                                .save_to_bufferv("png", &[])
                                                .unwrap(),
                                            ))
                                            .unwrap();
                                    }
                                };
                            }
                            Err(_) => {
                                sender
                                    .send(crate::ui::messages::Msg::DownloadImage(t, asset.clone()))
                                    .unwrap();
                            }
                        };
                    });
                }
            }
        }
    }

    fn main_window(&self) -> Option<&crate::window::EpicAssetManagerWindow> {
        let self_ = self.imp();
        match self_.window.get() {
            Some(window) => Some(&(*window)),
            None => None,
        }
    }

    pub fn fetch_assets(&self) {
        let self_ = self.imp();
        self.set_property("to-load", 0u32);
        self.set_property("loaded", 0u32);
        self_
            .refresh_progress
            .set_tooltip_text(Some("Loading from cache"));
        if let Some(window) = self.main_window() {
            let win_ = window.imp();
            let cache_dir = self_.settings.string("cache-directory").to_string();
            let cache_path = PathBuf::from(cache_dir);
            debug!("Fetching assets");
            if cache_path.is_dir() {
                debug!("Checking cache");
                let entries = std::fs::read_dir(cache_path).unwrap();
                for entry in entries {
                    let sender = win_.model.borrow().sender.clone();
                    self_.asset_load_pool.execute(move || {
                        // Load assets from cache

                        if let Ok(w) = crate::RUNNING.read() {
                            if !*w {
                                return;
                            }
                        }
                        let mut asset_file = entry.unwrap().path();
                        asset_file.push("asset_info.json");
                        if asset_file.exists() {
                            sender
                                .send(crate::ui::messages::Msg::StartAssetProcessing)
                                .unwrap();
                            if let Ok(f) = std::fs::File::open(asset_file.as_path()) {
                                if let Ok(asset) = serde_json::from_reader(f) {
                                    sender
                                        .send(crate::ui::messages::Msg::ProcessAssetInfo(asset))
                                        .unwrap();
                                }
                            };
                        }
                    });
                }
            };
            self.set_property("to-load", 0u32);
            self.set_property("loaded", 0u32);
            self_
                .refresh_progress
                .set_tooltip_text(Some("Loading from Epic Store"));
            let mut eg = win_.model.borrow().epic_games.borrow().clone();
            let sender = win_.model.borrow().sender.clone();
            // Start loading assets from the API
            self_.asset_load_pool.execute(move || {
                let assets = tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(eg.list_assets());
                for asset in assets {
                    sender
                        .send(crate::ui::messages::Msg::StartAssetProcessing)
                        .unwrap();
                    sender
                        .send(crate::ui::messages::Msg::ProcessEpicAsset(asset))
                        .unwrap();
                }
            });
            self.refresh_state_changed();
            glib::idle_add_local(clone!(@weak self as library => @default-panic, move || {
                glib::Continue(library.flush_loop())
            }));
        }
    }

    fn flush_loop(&self) -> bool {
        self.flush_assets();
        self.refresh_state_changed();
        !self.can_be_refreshed()
    }

    pub(crate) fn process_epic_asset(
        &self,
        epic_asset: &egs_api::api::types::epic_asset::EpicAsset,
    ) {
        let self_ = self.imp();
        if let Some(window) = self.main_window() {
            let win_ = window.imp();
            let mut cache_dir = PathBuf::from(self_.settings.string("cache-directory").to_string());
            cache_dir.push(&epic_asset.catalog_item_id);
            let mut cache_dir_c = cache_dir.clone();
            let ea = epic_asset.clone();

            // Write the Epic Asset file
            self_.asset_load_pool.execute(move || {
                cache_dir_c.push("epic_asset.json");
                fs::create_dir_all(cache_dir_c.parent().unwrap()).unwrap();
                if let Ok(mut asset_file) = File::create(cache_dir_c.as_path()) {
                    asset_file
                        .write_all(serde_json::to_string(&ea).unwrap().as_bytes().as_ref())
                        .unwrap();
                }
            });

            let mut eg = win_.model.borrow().epic_games.borrow().clone();
            let sender = win_.model.borrow().sender.clone();
            let mut cache_dir_c = cache_dir;
            let epic_asset = epic_asset.clone();
            self_.asset_load_pool.execute(move || {
                if let Ok(w) = crate::RUNNING.read() {
                    if !*w {
                        return;
                    }
                }
                if let Some(asset) = tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(eg.asset_info(epic_asset.clone()))
                {
                    // TODO: Check with already added assets to see if it needs updating
                    cache_dir_c.push("asset_info.json");
                    fs::create_dir_all(cache_dir_c.parent().unwrap()).unwrap();
                    if let Ok(mut asset_file) = File::create(cache_dir_c.as_path()) {
                        asset_file
                            .write_all(serde_json::to_string(&asset).unwrap().as_bytes().as_ref())
                            .unwrap();
                    }
                    sender
                        .send(crate::ui::messages::Msg::ProcessAssetInfo(asset))
                        .unwrap();
                }
            });
        }
    }

    pub fn refresh_asset(&self, id: &str) {
        let self_ = self.imp();
        if let Some(data) = self_.loaded_data.borrow().get(id) {
            data.refresh();
        }
        self.apply_filter();
    }
}

impl crate::ui::widgets::logged_in::refresh::Refresh for EpicLibraryBox {
    fn run_refresh(&self) {
        self.fetch_assets();
    }
    fn can_be_refreshed(&self) -> bool {
        let self_ = self.imp();
        self_.asset_load_pool.queued_count()
            + self_.asset_load_pool.active_count()
            + self_.image_load_pool.queued_count()
            + self_.image_load_pool.active_count()
            == 0
    }
    fn refresh_state_changed(&self) {
        let self_ = self.imp();
        if let Some(w) = self_.window.get() {
            let w_ = w.imp();
            w_.logged_in_stack.tab_switched();
        }
    }
}
