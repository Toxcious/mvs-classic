//! BP_SocialMenu (PreMatch/BP_SocialMenu): a player's click target on a level actor, its BannerClick component's
//! WBP_3dClick (SizeBox_1 300 x 500 over a 3D UI_Interactor, drawn with Widget3DPassThrough_Translucent: nothing
//! shows). The PreMatch level's BP_SocialMenu_3 rides BP_Banner_PM_Player2: the main menu party partner's
//! (main_menu::PartnerClick places it).

use std::any::Any;

use super::controls::interactor::set_interactable;
use super::controls::{bind, find};
use crate::menu::controller::{Controller, Cx};

const CLICKED: u32 = 1;

/// The actor's state on its WBP_3dClick: the account its click is for and the clicks taken.
#[derive(Debug, Default)]
pub struct SocialMenu {
    account_id: String,
    /// The account of each click taken (ShowContextMenu's), oldest first.
    clicks: Vec<String>,
}

impl SocialMenu {
    pub fn make(_cx: &mut Cx<'_>) -> Box<dyn Controller> {
        Box::new(SocialMenu::default())
    }

    /// Enable(Enable) @1102-1125: the interactor's SetInteractable.
    pub fn enable(&mut self, cx: &mut Cx<'_>, on: bool) {
        let i = find(cx, "UI_Interactor");
        set_interactable(cx, i, on);
    }

    /// Set Account ID.
    pub fn set_account_id(&mut self, id: &str) {
        self.account_id = id.to_string();
    }

    /// The clicks taken since the last take: the account each was for.
    pub fn take_clicks(&mut self) -> Vec<String> {
        std::mem::take(&mut self.clicks)
    }

    /// Clicked @353 -> ShowContextMenu @574-1056: ShowSocialActionsMenu(Account ID, BannerClick's screen position,
    /// false, "", self), then the account's modal as a UI_SocialContextMenu: Omit Actions [3, 7, 10] and Init with
    /// the account's SocialPlayerData.
    /// The click is kept here; the app opens the menu at the banner's projected point (menus.rs PartyPartner).
    fn show_context_menu(&mut self) {
        self.clicks.push(self.account_id.clone());
    }
}

impl Controller for SocialMenu {
    /// ReceiveBeginPlay @44-278 binds the interactor's three delegates: Hover and Unhover (@321, @337:
    /// BPI_HoverGlow(true / false), whose BP_SocialMenu body @320 is empty, so nothing to port) and Clicked.
    // Not ported: Update Widget Reported Positions Loop (@368-519, each 1 s: the interactor's Normalized Position
    // Override from BannerClick's screen place, for the pad's navigation); the port's interactor takes its place
    // from the layout.
    fn on_initialized(&mut self, cx: &mut Cx<'_>) {
        let i = find(cx, "UI_Interactor");
        bind(cx, i, |s| &mut s.on_click, CLICKED);
    }

    fn handle(&mut self, _cx: &mut Cx<'_>, id: u32, _arg: &dyn Any) {
        if id == CLICKED {
            self.show_context_menu();
        }
    }
}
