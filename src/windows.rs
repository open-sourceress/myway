#[derive(Debug, Default)]
pub enum WindowRole {
	#[default]
	Unassigned,
	Toplevel(ToplevelRole),
	Popup(PopupRole),
}

#[derive(Debug)]
pub struct ToplevelRole {
	pub title: Option<Box<str>>,
	pub app_id: Option<Box<str>>,
}

#[derive(Debug)]
pub struct PopupRole;
