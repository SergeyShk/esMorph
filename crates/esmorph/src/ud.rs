//! UD tags of the dictionary: strings are kept once, tags as numbers of strings

use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};

use crate::Error;
use crate::format::Strings;

/// Part of speech, features and labels of Universal Dependencies
#[derive(Clone, Copy)]
pub struct UdTag<'d> {
    strings: &'d [Box<str>],
    /// Part of speech, number of features, pairs of "name, value", number of labels, labels - as
    /// numbers of strings
    data: &'d [u16],
}

impl<'d> UdTag<'d> {
    fn string(self, number: u16) -> &'d str {
        &self.strings[usize::from(number)]
    }

    fn feat_numbers(self) -> &'d [u16] {
        &self.data[2..2 + 2 * usize::from(self.data[1])]
    }

    fn label_numbers(self) -> &'d [u16] {
        let start = 3 + 2 * usize::from(self.data[1]);
        &self.data[start..]
    }

    /// Part of speech
    pub fn pos(self) -> &'d str {
        self.string(self.data[0])
    }

    /// Features in the alphabetical order without regard to case, as in CoNLL-U: ("Gender", "Fem")
    pub fn feats(self) -> impl ExactSizeIterator<Item = (&'d str, &'d str)> {
        self.feat_numbers()
            .as_chunks::<2>()
            .0
            .iter()
            .map(move |&[name, value]| (self.string(name), self.string(value)))
    }

    /// Value of the feature `name`
    pub fn feat(self, name: &str) -> Option<&'d str> {
        self.feats()
            .find(|&(known, _)| known == name)
            .map(|(_, value)| value)
    }

    /// Labels of the dictionary that have no UD feature
    pub fn labels(self) -> impl ExactSizeIterator<Item = &'d str> {
        self.label_numbers()
            .iter()
            .map(move |&number| self.string(number))
    }

    /// Features in CoNLL-U notation: "Gender=Fem|Number=Sing"
    pub fn feats_string(self) -> String {
        self.feats()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("|")
    }
}

impl PartialEq for UdTag<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.pos() == other.pos()
            && self.feats().eq(other.feats())
            && self.labels().eq(other.labels())
    }
}

impl Eq for UdTag<'_> {}

impl Hash for UdTag<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pos().hash(state);
        self.feats().len().hash(state);
        self.feats().for_each(|feat| feat.hash(state));
        self.labels().for_each(|label| label.hash(state));
    }
}

impl fmt::Debug for UdTag<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UdTag")
            .field("pos", &self.pos())
            .field("feats", &self.feats().collect::<Vec<_>>())
            .field("labels", &self.labels().collect::<Vec<_>>())
            .finish()
    }
}

/// UD tags of a dictionary
#[derive(Clone)]
pub(crate) struct UdTags {
    /// Different strings of the tags: equal strings of a file get one number
    strings: Vec<Box<str>>,
    /// Start of every tag in `data` and the common end
    bounds: Vec<u32>,
    data: Vec<u16>,
}

impl UdTags {
    /// Tags from a table of strings and tags in a row as numbers of strings
    pub fn read(table: &Strings, data: &[u16]) -> Result<Self, Error> {
        let invalid = |msg: &str| Err(Error::Format(msg.into()));
        let mut tags = UdTags {
            strings: Vec::new(),
            bounds: vec![0],
            data: Vec::with_capacity(data.len()),
        };
        let mut numbers: HashMap<&str, u16> = HashMap::new();
        let mut renumbered = Vec::with_capacity(table.len());
        for text in table.iter() {
            let next = numbers.len() as u16;
            let number = *numbers.entry(text).or_insert(next);
            if number == next {
                tags.strings.push(text.into());
            }
            renumbered.push(number);
        }
        let mut at = 0;
        while at < data.len() {
            let Some(&count) = data.get(at + 1) else {
                return invalid("a UD tag is truncated");
            };
            let labels = at + 2 + 2 * usize::from(count);
            let Some(&label_count) = data.get(labels) else {
                return invalid("a UD tag is truncated");
            };
            let end = labels + 1 + usize::from(label_count);
            let Some(tag) = data.get(at..end) else {
                return invalid("a UD tag is truncated");
            };
            for (index, &number) in tag.iter().enumerate() {
                let is_count = index == 1 || index == labels - at;
                tags.data.push(if is_count {
                    number
                } else {
                    match renumbered.get(usize::from(number)) {
                        Some(&number) => number,
                        None => return invalid("a UD tag refers outside the table of strings"),
                    }
                });
            }
            if tags.strings[usize::from(tags.data[at])].is_empty() {
                return invalid("a UD tag without a part of speech");
            }
            tags.bounds.push(end as u32);
            at = end;
        }
        if tags.len() > usize::from(u16::MAX) + 1 {
            return invalid("more than 65 536 UD tags");
        }
        Ok(tags)
    }

    pub fn len(&self) -> usize {
        self.bounds.len() - 1
    }

    pub fn get(&self, number: u16) -> Option<UdTag<'_>> {
        let number = usize::from(number);
        (number < self.len()).then(|| self.tag(number))
    }

    fn tag(&self, number: usize) -> UdTag<'_> {
        let (start, end) = (self.bounds[number], self.bounds[number + 1]);
        UdTag {
            strings: &self.strings,
            data: &self.data[start as usize..end as usize],
        }
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = UdTag<'_>> {
        (0..self.len()).map(|number| self.tag(number))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(strings: &[&str]) -> Strings {
        let mut bytes = (strings.len() as u32).to_le_bytes().to_vec();
        let mut end = 0;
        for text in strings {
            end += text.len() as u32;
            bytes.extend_from_slice(&end.to_le_bytes());
        }
        for text in strings {
            bytes.extend_from_slice(text.as_bytes());
        }
        Strings::read(&bytes).unwrap()
    }

    #[test]
    fn reads_tags() {
        let strings = table(&["NOUN", "Gender", "Fem", "vos", "NOUN", "VERB"]);
        let tags = UdTags::read(&strings, &[0, 1, 1, 2, 1, 3, 5, 0, 0, 4, 0, 0]).unwrap();
        assert_eq!(tags.len(), 3);
        let noun = tags.get(0).unwrap();
        assert_eq!(noun.pos(), "NOUN");
        assert_eq!(noun.feats().collect::<Vec<_>>(), [("Gender", "Fem")]);
        assert_eq!(noun.feat("Gender"), Some("Fem"));
        assert_eq!(noun.feat("Number"), None);
        assert_eq!(noun.feats_string(), "Gender=Fem");
        assert_eq!(noun.labels().collect::<Vec<_>>(), ["vos"]);
        assert_eq!(tags.get(1).unwrap().pos(), "VERB");
        assert_eq!(tags.get(1).unwrap().feats().len(), 0);
        let unique: std::collections::HashSet<UdTag> = tags.iter().chain(tags.iter()).collect();
        assert_eq!(unique.len(), 3);
        assert!(tags.get(3).is_none());
        assert_eq!(
            format!("{:?}", tags.get(1).unwrap()),
            r#"UdTag { pos: "VERB", feats: [], labels: [] }"#
        );
    }

    #[test]
    fn rejects_broken_tags() {
        let strings = table(&["NOUN", "", "Fem"]);
        for data in [
            &[0][..],
            &[0, 1, 1],
            &[0, 1, 1, 2],
            &[0, 0, 1],
            &[0, 0, 1, 7],
            &[3, 0, 0],
            &[1, 0, 0],
        ] {
            assert!(UdTags::read(&strings, data).is_err(), "{data:?}");
        }
        assert!(UdTags::read(&strings, &[0, 0, 1, 2]).is_ok());
    }
}
