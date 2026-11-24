//! What a cascade builds once from its rules for each quirks mode: the
//! buckets, the layer ranks and the selectors' features.

use super::buckets::Buckets;
use super::features::Features;
use super::rules::Rules;

pub(crate) struct Prepared {
    pub quirks: bool,
    pub buckets: Buckets,
    pub ranks: Vec<u32>,
    /// The rank of the author origin's unlayered rules
    pub author_rank: u32,
    pub features: Features,
}

impl Prepared {
    pub fn new(rules: &Rules<'_>, quirks: bool) -> Prepared {
        Prepared {
            quirks,
            buckets: Buckets::build(rules, quirks),
            ranks: rules.layer_ranks(),
            author_rank: rules.author_rank(),
            features: Features::build(rules),
        }
    }
}
