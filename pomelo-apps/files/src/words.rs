//! The page's words, in both languages.
//!
//! Every string a person reads lives here and nowhere else, so that adding a language is adding an
//! arm to [`Words::of`] rather than hunting through a view for a literal. The one that takes an
//! argument — the question a removal asks — is a method for the same reason: a name in the middle of
//! a sentence is where a language's word order stops being a substitution.

use pomelo_widgets::preferences::Language;

/// What the page says, in one language.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Words {
    /// The language these words are in, for the one phrase that is built rather than picked.
    pub(crate) language: Language,

    /// The built-in volume.
    pub(crate) internal: &'static str,
    /// The card in the slot.
    pub(crate) card: &'static str,
    /// Where a card would be, when there is not one.
    pub(crate) no_card: &'static str,

    /// Look for the card again, and list the directory again.
    pub(crate) refresh: &'static str,

    /// The four actions.
    pub(crate) new_folder: &'static str,
    pub(crate) rename: &'static str,
    pub(crate) copy: &'static str,
    pub(crate) paste: &'static str,
    pub(crate) delete: &'static str,

    /// The prompt's three titles.
    pub(crate) ask_new: &'static str,
    pub(crate) ask_rename: &'static str,
    /// The placeholder in the name box.
    pub(crate) hint: &'static str,

    /// The prompt's two buttons.
    pub(crate) okay: &'static str,
    pub(crate) cancel: &'static str,

    /// A directory with nothing in it.
    pub(crate) empty: &'static str,
}

impl Words {
    /// The words for a language.
    pub(crate) fn of(language: Language) -> Self {
        match language {
            Language::Chinese => Self {
                language,
                internal: "内部",
                card: "存储卡",
                no_card: "没有插卡",
                refresh: "重新查找存储卡",
                new_folder: "新建文件夹",
                rename: "重命名",
                copy: "复制",
                paste: "粘贴",
                delete: "删除",
                ask_new: "新建文件夹",
                ask_rename: "重命名",
                hint: "名称",
                okay: "确定",
                cancel: "取消",
                empty: "这个文件夹是空的",
            },
            Language::English => Self {
                language,
                internal: "Internal",
                card: "Card",
                no_card: "no card in the slot",
                refresh: "Look for the card again",
                new_folder: "New folder",
                rename: "Rename",
                copy: "Copy",
                paste: "Paste",
                delete: "Delete",
                ask_new: "New folder",
                ask_rename: "Rename",
                hint: "name",
                okay: "OK",
                cancel: "Cancel",
                empty: "Nothing in this folder",
            },
        }
    }

    /// What to call the volume at `index`: the built-in one is first, a card after it.
    pub(crate) fn volume(&self, index: usize) -> &'static str {
        if index == 0 {
            self.internal
        } else {
            self.card
        }
    }

    /// The question a removal asks, with the name in it.
    ///
    /// Built per language rather than substituted into one pattern, because the two put the name in
    /// different places and a `"{}"` shared between them would read like a machine one time in two.
    pub(crate) fn ask_delete(&self, name: &str) -> String {
        match self.language {
            Language::Chinese => format!("删除「{name}」？"),
            Language::English => format!("Delete \"{name}\"?"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_languages_say_something_for_everything() {
        for language in [Language::Chinese, Language::English] {
            let words = Words::of(language);

            for saying in [
                words.internal,
                words.card,
                words.no_card,
                words.refresh,
                words.new_folder,
                words.rename,
                words.copy,
                words.paste,
                words.delete,
                words.ask_new,
                words.ask_rename,
                words.hint,
                words.okay,
                words.cancel,
                words.empty,
            ] {
                assert!(!saying.trim().is_empty(), "{language:?} left a line blank");
            }
        }
    }

    #[test]
    fn the_two_languages_are_not_the_same_words() {
        let chinese = Words::of(Language::Chinese);
        let english = Words::of(Language::English);

        assert_ne!(chinese.delete, english.delete);
        assert_ne!(chinese.ask_delete("a.txt"), english.ask_delete("a.txt"));
    }

    #[test]
    fn the_first_volume_is_the_one_inside_the_box() {
        let words = Words::of(Language::English);

        assert_eq!(words.volume(0), words.internal);
        assert_eq!(words.volume(1), words.card, "and a second one is the card");
    }

    #[test]
    fn a_removal_question_carries_the_name() {
        let words = Words::of(Language::English);

        assert!(words.ask_delete("holiday.jpg").contains("holiday.jpg"));
    }
}
