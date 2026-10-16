//! Info > Library Packages and Library Padstacks: the rows of Java's
//! `WindowPackages` and `WindowPadstacks` (G04.2).
//!
//! Both list the library sorted by name without regard to case. Select takes
//! the items of every component that uses a chosen package (the package for
//! the side it is placed on), or every pin and via that uses a chosen
//! padstack.

use freerouting::board::BasicBoard;
use freerouting::board::items::ItemId;

use super::object_list::Row;

fn sorted_by_name<T>(mut list: Vec<T>, name: impl Fn(&T) -> &str) -> Vec<T> {
    list.sort_by_key(|x| name(x).to_lowercase());
    list
}

/// The rows of the packages list. The items of a package are those of the
/// components placed with it.
pub fn package_rows(board: &BasicBoard) -> Vec<Row> {
    let packages = sorted_by_name(board.library.packages.iter().collect(), |p| &p.name);
    packages
        .into_iter()
        .map(|package| {
            let mut items: Vec<ItemId> = Vec::new();
            for component in board
                .components
                .iter()
                .filter(|c| c.package() == package.id)
            {
                items.extend(board.component_items(component.id.0));
            }
            items.sort_unstable();
            items.dedup();
            Row {
                text: package.name.clone(),
                items,
                detail: None,
                filter_text: None,
            }
        })
        .collect()
}

/// The rows of the padstacks list. The items of a padstack are the pins and
/// vias that use it.
pub fn padstack_rows(board: &BasicBoard) -> Vec<Row> {
    let padstacks = sorted_by_name(board.library.padstacks.iter().collect(), |p| &p.name);
    padstacks
        .into_iter()
        .map(|padstack| {
            let items: Vec<ItemId> = board
                .items()
                .filter(|item| item.drill().is_some_and(|d| d.padstack == padstack.id))
                .map(|item| item.id())
                .collect();
            Row {
                text: padstack.name.clone(),
                items,
                detail: None,
                filter_text: None,
            }
        })
        .collect()
}
