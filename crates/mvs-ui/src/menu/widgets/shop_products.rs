//! The port's stand-in for the shop's native ShopManager and its product list, over the menu bundle's shop catalogue
//! (`bundle::ShopCatalogue`), and the shop grid's packer (UI_WWShopProductGridGeneric Reserve Layout Space with
//! WWGridSlot).
//!
//! Deviation: the build asks Hydra for its products (ShopManager.FetchAllProducts, FetchProductsWithAnyTags,
//! FetchProductFromProductSlug) and Hydra sends the tags, prices, sales and ownership. Without a backend the port
//! answers at once from the import's catalogue: a tab's tags stand for the asset classes, the bundles carry their
//! gem price, a single item has none, nothing is owned and the balance of every currency is 0, so nothing can be
//! bought.

use crate::bundle::{ShopBundle, ShopCatalogue, ShopItem};

/// ECurrencyType's bytes.
pub const CURRENCY_GOLD: u8 = 0;
pub const CURRENCY_GEMS: u8 = 2;
/// The cell type of a bundle (a product with no synced asset and more than one BundleProducts).
pub const KIND_BUNDLE: u32 = 16;

/// A ShopProduct as the shop's widgets read it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShopProduct {
    /// ProductSlug.
    pub slug: String,
    /// The cell type: the synced asset's Get Item Type, [`KIND_BUNDLE`] for a bundle.
    pub kind: u32,
    /// DisplayName.
    pub name: String,
    /// A bundle's BundleSubText, else its DisplayName; empty for an item.
    pub sub_text: String,
    /// ERewardRarity's tail (`None` when unset).
    pub rarity: String,
    /// AssociatedCharacter's EFighter tail.
    pub fighter: Option<String>,
    /// AssociatedUniverse's tail.
    pub universe: Option<String>,
    /// The stand-in's tags (the catalogue's tag; `bundle` for a bundle).
    pub tags: Vec<String>,
    /// CurrencyCosts: (ECurrencyType byte, amount).
    pub costs: Vec<(u8, f64)>,
    /// BundleProducts: the bundle's items' slugs.
    pub bundle_products: Vec<String>,
    /// The cell's picture: a material or texture object path.
    pub image: Option<String>,
    /// A bundle's RequiresAllOwned.
    pub requires_all_owned: bool,
}

impl ShopProduct {
    /// BundleProducts length > 1.
    pub fn is_bundle(&self) -> bool {
        self.bundle_products.len() > 1
    }

    /// The price in currency `c` (CurrencyCosts' entry), None when there is none.
    pub fn cost(&self, c: u8) -> Option<f64> {
        self.costs.iter().find(|x| x.0 == c).map(|x| x.1)
    }
}

/// ECurrencyType's byte of its tail (`gems`); None for another name.
pub fn currency_byte(name: &str) -> Option<u8> {
    match name {
        "Gold" => Some(0),
        "FriendPoints" => Some(1),
        "gems" => Some(2),
        "CharTickets" => Some(3),
        _ => None,
    }
}

/// ERewardRarity's byte of its tail: 0 for `None` and an unknown name.
pub fn rarity_byte(name: &str) -> u8 {
    match name {
        "Common" => 1,
        "Rare" => 2,
        "Epic" => 3,
        "Legendary" => 4,
        _ => 0,
    }
}

/// The product of a single item: no price (the build's prices are Hydra's).
pub fn item_product(i: &ShopItem) -> ShopProduct {
    ShopProduct {
        slug: i.slug.clone(),
        kind: i.kind,
        name: i.name.clone(),
        sub_text: String::new(),
        rarity: i.rarity.clone(),
        fighter: i.fighter.clone(),
        universe: i.universe.clone(),
        tags: if i.tag.is_empty() { Vec::new() } else { vec![i.tag.clone()] },
        costs: Vec::new(),
        bundle_products: Vec::new(),
        image: i.image.clone(),
        requires_all_owned: false,
    }
}

/// The product of a bundle: its gem price, its items, type 16 when it holds more than one.
pub fn bundle_product(b: &ShopBundle) -> ShopProduct {
    let items: Vec<String> = b.items.iter().map(|i| i.slug.clone()).collect();
    let sub = b.sub_text.as_ref().map(|t| t.text.clone()).filter(|t| !t.is_empty());
    ShopProduct {
        slug: b.slug.clone(),
        kind: if items.len() > 1 { KIND_BUNDLE } else { 0 },
        name: b.name.text.clone(),
        sub_text: sub.unwrap_or_else(|| b.name.text.clone()),
        rarity: "None".to_string(),
        fighter: b.fighter.clone(),
        universe: b.universe.clone(),
        tags: vec!["bundle".to_string()],
        costs: b.prices.iter().filter_map(|p| Some((currency_byte(&p.currency)?, p.amount))).collect(),
        bundle_products: items,
        image: b.cell_material.clone(),
        requires_all_owned: b.requires_all_owned,
    }
}

/// FetchAllProducts: every bundle, then every listed item.
// Unverified: the order of Hydra's answer is not in the build; the bundles first, then the catalogue's order.
pub fn fetch_all_products(cat: &ShopCatalogue) -> Vec<ShopProduct> {
    let mut v: Vec<ShopProduct> = cat.bundles.iter().map(bundle_product).collect();
    v.extend(cat.items.iter().filter(|i| i.listed).map(item_product));
    v
}

/// FetchProductsWithAnyTags: the products with any of `tags`, in the catalogue's order.
pub fn fetch_products_with_any_tags(cat: &ShopCatalogue, tags: &[&str]) -> Vec<ShopProduct> {
    fetch_all_products(cat).into_iter().filter(|p| p.tags.iter().any(|t| tags.contains(&t.as_str()))).collect()
}

/// FetchProductFromProductSlug: the bundle, else the item (listed or a bundle's).
pub fn fetch_product_from_slug(cat: &ShopCatalogue, slug: &str) -> Option<ShopProduct> {
    if let Some(b) = cat.bundles.iter().find(|b| b.slug == slug) {
        return Some(bundle_product(b));
    }
    cat.item(slug).map(item_product)
}

/// GetBundleProductsArray: the products of a bundle's items that the catalogue holds.
pub fn bundle_products_array(cat: &ShopCatalogue, p: &ShopProduct) -> Vec<ShopProduct> {
    p.bundle_products.iter().filter_map(|s| cat.item(s)).map(item_product).collect()
}

/// GetAssociatedBundles (NQ5): the bundles that hold the item `slug`.
// Unverified: native; the bundles whose items name the slug, in the catalogue's order.
pub fn associated_bundles(cat: &ShopCatalogue, slug: &str) -> Vec<ShopProduct> {
    cat.bundles_with(slug).map(bundle_product).collect()
}

/// FilterFeaturedTabProducts (NQ2): (WeeklySpecials, NewReleases).
// Deviation: the split is native and reads Hydra's fields. The port's choice: the fighter bundles (`bundle_`) are
// the specials, the dated bundles (`20230214_`) the new releases; no item goes to either.
pub fn split_featured(products: &[ShopProduct]) -> (Vec<ShopProduct>, Vec<ShopProduct>) {
    let pick = |prefix: &str| products.iter().filter(|p| p.slug.starts_with(prefix)).cloned().collect::<Vec<_>>();
    (pick("bundle_"), pick("20230214_"))
}

/// ShopProductIsOwned: IsOwned || !CheckSucceeded.
// Deviation: the port has no inventory: nothing is owned, so every card is offered and its details open.
pub fn is_owned(_p: &ShopProduct) -> bool {
    false
}

/// Get Client Balance In Currency (NQ7).
// Deviation: no wallet: every balance is 0, which keeps every purchase short of funds.
pub fn client_balance(_currency: u8) -> f64 {
    0.0
}

/// PurchaseProduct.
// Deviation: buying is off: the request fails at once (the 0 balance keeps the confirm buttons from reaching it).
pub fn purchase_product(_p: &ShopProduct, _currency: u8) -> bool {
    false
}

/// ESortingOrder's bytes the filter grid offers.
pub const SORT_ALPHABETICAL: u8 = 0;
pub const SORT_CHARACTER: u8 = 1;
pub const SORT_NEWEST: u8 = 2;
pub const SORT_RARITY: u8 = 5;
pub const SORT_COST: u8 = 7;

/// SortAndFilterProducts (NQ1) by the filter grid's sort and universe: the products of `universe` ("All": every
/// one), sorted by `order`.
// Unverified: native. Alphabetical by name, Character by the fighter, Rarity from Legendary down; Newest and Cost
// keep the catalogue's order (no dates, no prices); a stable sort, so ties keep it too.
pub fn sort_and_filter(products: &[ShopProduct], order: u8, universe: &str) -> Vec<ShopProduct> {
    let mut v: Vec<ShopProduct> =
        products.iter().filter(|p| universe == "All" || p.universe.as_deref() == Some(universe)).cloned().collect();
    match order {
        SORT_ALPHABETICAL => v.sort_by_key(|p| p.name.to_lowercase()),
        SORT_CHARACTER => v.sort_by_key(|p| p.fighter.clone().unwrap_or_default().to_lowercase()),
        SORT_RARITY => v.sort_by_key(|p| std::cmp::Reverse(rarity_byte(&p.rarity))),
        _ => {}
    }
    v
}

// ---------- the grid's packer ----------

/// UI_WWShopProductGridGeneric's Mosiac Sizes (the class default), the cell sizes of the bundles tab in turn.
pub const MOSAIC_SIZES: [(i32, i32); 10] =
    [(30, 2), (12, 2), (18, 2), (10, 2), (10, 2), (10, 2), (15, 2), (15, 2), (10, 2), (20, 2)];

/// Make Mosaic Sizes(Amount): the keys 0..=Amount, each the next size of [`MOSAIC_SIZES`] (wrapping).
pub fn mosaic_sizes(amount: usize) -> Vec<(i32, i32)> {
    (0..=amount).map(|i| MOSAIC_SIZES[i % MOSAIC_SIZES.len()]).collect()
}

/// A product's place in the grid (WWGridSlot): column, row, width and height in grid squares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridSlot {
    /// The product's index in the list given.
    pub index: usize,
    pub col: i32,
    pub row: i32,
    pub w: i32,
    pub h: i32,
}

/// WWGridSlot Recieve Product: Width 6, Height 1, 2 for types 1, 15 and 16 (Get Product Height Multiplier); a custom
/// size takes their place; the width is then clamped to 6..Max Columns and the height kept at 1 or more.
pub fn slot_size(kind: u32, custom: Option<(i32, i32)>, max_cols: i32) -> (i32, i32) {
    let (w, h) = custom.unwrap_or((6, if matches!(kind, 1 | 15 | 16) { 2 } else { 1 }));
    // UE Clamp: Min(Max(v, lo), hi)
    (w.max(6).min(max_cols), h.max(1))
}

/// The grid's packed slots and the totals (Total Occupied Columns, Total Occupied Rows).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Packed {
    pub slots: Vec<GridSlot>,
    pub cols: i32,
    pub rows: i32,
}

/// Grid Is Occupied: past the right edge (Column + Width - 1 > Max Width, as the build tests it), or any square of
/// the area taken.
fn occupied(col: i32, row: i32, w: i32, h: i32, taken: &[(i32, i32)], max_cols: i32) -> bool {
    if col + w - 1 > max_cols {
        return true;
    }
    (col..col + w).any(|c| (row..row + h).any(|r| taken.contains(&(c, r))))
}

/// Reserve Layout Space: each slot in turn goes to the first free place of a walker that moves one column at a time
/// (and to the next row past the last column); the walker does not move after the last slot. The highest column and
/// row reached (the walker's included) make the totals. With `fill_row` (the bundles tab) a second pass stretches
/// each slot whose next slot starts another row to the last column (Stretch Cell To End) and packs again; the
/// highest row of the first pass is kept.
pub fn pack(sizes: &[(i32, i32)], max_cols: i32, fill_row: bool) -> Packed {
    let max_cols = max_cols.max(1);
    let mut local: Vec<GridSlot> =
        sizes.iter().enumerate().map(|(index, &(w, h))| GridSlot { index, col: 0, row: 0, w, h }).collect();
    let mut second = fill_row;
    let mut max_r = 0;
    let mut last_mark = (0, 0);
    loop {
        let mut max_c = 0;
        let (mut col, mut row) = (0, 0);
        let mut taken: Vec<(i32, i32)> = Vec::new();
        let mut done: Vec<GridSlot> = Vec::new();
        while !local.is_empty() {
            local[0].w = local[0].w.min(max_cols);
            let s = local[0];
            if !occupied(col, row, s.w, s.h, &taken, max_cols) {
                // Mark Grid Space Occupied: its Max Occupied C / R are of this area only
                let (mut mc, mut mr) = (0, 0);
                for c in col..col + s.w {
                    for r in row..row + s.h {
                        if !taken.contains(&(c, r)) {
                            taken.push((c, r));
                        }
                        mc = mc.max(c);
                        mr = mr.max(r);
                    }
                }
                last_mark = (mc, mr);
                done.push(GridSlot { col, row, ..s });
                local.remove(0);
            }
            if !local.is_empty() {
                col += 1;
                if col >= max_cols {
                    col = 0;
                    row += 1;
                }
            }
            max_c = max_c.max(col).max(last_mark.0);
            max_r = max_r.max(row).max(last_mark.1);
        }
        if !second {
            return Packed { slots: done, cols: max_c + 1, rows: max_r + 1 };
        }
        second = false;
        local = stretch_cell_to_end(done, max_cols);
    }
}

/// Stretch Cell To End: a slot whose next slot sits on another row takes Max Columns - its column as its width (the
/// last slot is left as it is).
fn stretch_cell_to_end(mut cells: Vec<GridSlot>, max_cols: i32) -> Vec<GridSlot> {
    for i in 1..cells.len() {
        if cells[i - 1].row != cells[i].row {
            cells[i - 1].w = max_cols - cells[i - 1].col;
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::{ShopBundleItem, ShopPrice, ShopText};

    fn item(slug: &str, kind: u32, tag: &str, listed: bool) -> ShopItem {
        ShopItem {
            slug: slug.into(),
            kind,
            tag: tag.into(),
            name: slug.to_uppercase(),
            rarity: "Rare".into(),
            listed,
            ..ShopItem::default()
        }
    }

    fn catalogue() -> ShopCatalogue {
        let bundle = |slug: &str, items: &[&str]| ShopBundle {
            slug: slug.into(),
            name: ShopText { text: slug.into(), ..ShopText::default() },
            prices: vec![ShopPrice { currency: "gems".into(), amount: 1500.0 }],
            items: items.iter().map(|s| ShopBundleItem { slug: (*s).into(), quantity: 1.0 }).collect(),
            ..ShopBundle::default()
        };
        ShopCatalogue {
            items: vec![
                item("skin_a", 1, "skin", true),
                item("taunt_a", 2, "taunt", true),
                item("emote_a", 8, "emote", true),
                item("skin_hidden", 1, "skin", false),
            ],
            bundles: vec![
                bundle("bundle_a", &["skin_a", "skin_hidden"]),
                bundle("20230214_x", &["taunt_a", "emote_a"]),
            ],
            ..ShopCatalogue::default()
        }
    }

    #[test]
    fn the_stand_in_answers_from_the_catalogue() {
        let c = catalogue();
        let all = fetch_all_products(&c);
        let slugs: Vec<&str> = all.iter().map(|p| p.slug.as_str()).collect();
        assert_eq!(slugs, ["bundle_a", "20230214_x", "skin_a", "taunt_a", "emote_a"]);
        let emotes = fetch_products_with_any_tags(&c, &["taunt", "emote"]);
        assert_eq!(emotes.iter().map(|p| p.slug.as_str()).collect::<Vec<_>>(), ["taunt_a", "emote_a"]);
        let bundles = fetch_products_with_any_tags(&c, &["bundle"]);
        assert_eq!(bundles.len(), 2);
        assert!(bundles[0].is_bundle());
        assert_eq!(bundles[0].kind, KIND_BUNDLE);
        assert_eq!(bundles[0].cost(CURRENCY_GEMS), Some(1500.0));
        // an unlisted item is found by its slug (a bundle's details), with no price
        let hidden = fetch_product_from_slug(&c, "skin_hidden").expect("a bundle's item");
        assert!(hidden.costs.is_empty());
        assert_eq!(bundle_products_array(&c, &bundles[0]).len(), 2);
        assert_eq!(associated_bundles(&c, "skin_hidden")[0].slug, "bundle_a");
        let (specials, news) = split_featured(&all);
        assert_eq!((specials[0].slug.as_str(), news[0].slug.as_str()), ("bundle_a", "20230214_x"));
    }

    #[test]
    fn nothing_is_owned_and_nothing_can_be_bought() {
        let c = catalogue();
        let b = fetch_product_from_slug(&c, "bundle_a").expect("a bundle");
        assert!(!is_owned(&b));
        assert_eq!(client_balance(CURRENCY_GEMS), 0.0);
        assert!(client_balance(CURRENCY_GEMS) - b.cost(CURRENCY_GEMS).unwrap_or(0.0) < 0.0);
        assert!(!purchase_product(&b, CURRENCY_GEMS));
    }

    #[test]
    fn the_sort_and_the_universe_filter() {
        let mut ps: Vec<ShopProduct> = ["b", "a", "c"]
            .iter()
            .map(|s| ShopProduct { slug: (*s).into(), name: (*s).into(), ..ShopProduct::default() })
            .collect();
        ps[0].rarity = "Legendary".into();
        ps[1].universe = Some("DC".into());
        let names = |v: Vec<ShopProduct>| v.into_iter().map(|p| p.slug).collect::<Vec<_>>();
        assert_eq!(names(sort_and_filter(&ps, SORT_ALPHABETICAL, "All")), ["a", "b", "c"]);
        assert_eq!(names(sort_and_filter(&ps, SORT_RARITY, "All")), ["b", "a", "c"]);
        assert_eq!(names(sort_and_filter(&ps, SORT_NEWEST, "DC")), ["a"]);
    }

    #[test]
    fn slot_sizes_follow_the_type() {
        assert_eq!(slot_size(1, None, 24), (6, 2));
        assert_eq!(slot_size(16, None, 24), (6, 2));
        assert_eq!(slot_size(2, None, 24), (6, 1));
        assert_eq!(slot_size(16, Some((30, 2)), 30), (30, 2));
        assert_eq!(slot_size(16, Some((30, 2)), 18), (18, 2));
        assert_eq!(slot_size(16, Some((4, 0)), 18), (6, 1));
    }

    #[test]
    fn the_packer_fills_rows_left_to_right() {
        // five 6 x 2 cells in 24 columns: four on rows 0-1, the fifth on rows 2-3
        let p = pack(&[(6, 2); 5], 24, false);
        let at: Vec<(i32, i32)> = p.slots.iter().map(|s| (s.col, s.row)).collect();
        assert_eq!(at, [(0, 0), (6, 0), (12, 0), (18, 0), (0, 2)]);
        assert_eq!((p.cols, p.rows), (24, 4));
        // two cells: the walker stops after the last, the totals are what the cells take
        let p = pack(&[(6, 1); 2], 30, false);
        assert_eq!((p.cols, p.rows), (12, 1));
        // an empty list
        let p = pack(&[], 30, false);
        assert_eq!((p.slots.len(), p.cols, p.rows), (0, 1, 1));
    }

    #[test]
    fn the_bundles_tab_packs_its_mosaic_and_fills_each_row() {
        let sizes = mosaic_sizes(4);
        let p = pack(&sizes[..4], 30, true);
        let s: Vec<(i32, i32, i32)> = p.slots.iter().map(|s| (s.col, s.row, s.w)).collect();
        // 30 | 12 18 | 10 (the last cell is not stretched)
        assert_eq!(s, [(0, 0, 30), (0, 2, 12), (12, 2, 18), (0, 4, 10)]);
        assert_eq!(p.rows, 6);
        // a row a cell short is stretched to the end: 12 then 20 (does not fit), so 12 takes the row
        let p = pack(&[(12, 2), (20, 2)], 30, true);
        let s: Vec<(i32, i32, i32)> = p.slots.iter().map(|s| (s.col, s.row, s.w)).collect();
        assert_eq!(s, [(0, 0, 30), (0, 2, 20)]);
    }
}
