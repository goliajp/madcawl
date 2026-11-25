//! What a cascade builds once from its rules for each quirks mode: the
//! buckets, the layer ranks and the selectors' features.

use super::ancestors::{Filter, needs};
use super::buckets::{Buckets, declares_content};
use super::features::Features;
use super::rules::Rules;

pub(crate) struct Prepared {
    pub quirks: bool,
    pub buckets: Buckets,
    /// The rules for `::before` and `::after`, by the rest of their rightmost compound
    pub pseudo_buckets: Buckets,
    /// Those of them that declare `content` (without one matching, a pseudo-element has no box) and need
    /// no more than the element and its ancestors (`Features::reval`), and those that need more
    pub content_buckets: Buckets,
    pub reval_content_buckets: Buckets,
    /// The pseudo-element rules that need more than the element and its ancestors
    pub reval_pseudo_buckets: Buckets,
    pub ranks: Vec<u32>,
    /// The rank of the author origin's unlayered rules
    pub author_rank: u32,
    pub features: Features,
    /// By rule, what some ancestor of an element it matches has
    pub needs: Vec<Filter>,
}

impl Prepared {
    pub fn new(rules: &Rules<'_>, quirks: bool) -> Prepared {
        let features = Features::build(rules);
        let all = |_| true;
        Prepared {
            quirks,
            buckets: Buckets::build(rules, quirks, false, all),
            pseudo_buckets: Buckets::build(rules, quirks, true, all),
            content_buckets: Buckets::build(rules, quirks, true, |i| {
                !features.reval[i] && declares_content(rules, i)
            }),
            reval_content_buckets: Buckets::build(rules, quirks, true, |i| {
                features.reval[i] && declares_content(rules, i)
            }),
            reval_pseudo_buckets: Buckets::build(rules, quirks, true, |i| features.reval[i]),
            ranks: rules.layer_ranks(),
            author_rank: rules.author_rank(),
            features,
            needs: needs(rules, quirks),
        }
    }
}
