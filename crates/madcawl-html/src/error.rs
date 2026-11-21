//! Parse errors, named by the codes the HTML standard gives them.

macro_rules! errors {
    ($($variant:ident => $code:literal,)*) => {
        /// A parse error. The parser recovers from every one of them;
        /// they are reported for tools, never acted on.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum ParseError {
            $($variant,)*
        }

        impl ParseError {
            /// The standard's code for it, as `eof-in-tag`.
            pub fn code(self) -> &'static str {
                match self {
                    $(ParseError::$variant => $code,)*
                }
            }
        }
    };
}

errors! {
    AbruptClosingOfEmptyComment => "abrupt-closing-of-empty-comment",
    AbruptDoctypePublicIdentifier => "abrupt-doctype-public-identifier",
    AbruptDoctypeSystemIdentifier => "abrupt-doctype-system-identifier",
    AbsenceOfDigitsInNumericCharacterReference => "absence-of-digits-in-numeric-character-reference",
    CdataInHtmlContent => "cdata-in-html-content",
    CharacterReferenceOutsideUnicodeRange => "character-reference-outside-unicode-range",
    ControlCharacterInInputStream => "control-character-in-input-stream",
    ControlCharacterReference => "control-character-reference",
    DisallowedProcessingInstructionTarget => "disallowed-processing-instruction-target",
    DuplicateAttribute => "duplicate-attribute",
    EndTagWithAttributes => "end-tag-with-attributes",
    EndTagWithTrailingSolidus => "end-tag-with-trailing-solidus",
    EofBeforeTagName => "eof-before-tag-name",
    EofInCdata => "eof-in-cdata",
    EofInComment => "eof-in-comment",
    EofInDoctype => "eof-in-doctype",
    EofInProcessingInstruction => "eof-in-processing-instruction",
    EofInScriptHtmlCommentLikeText => "eof-in-script-html-comment-like-text",
    EofInTag => "eof-in-tag",
    IncorrectlyClosedComment => "incorrectly-closed-comment",
    IncorrectlyOpenedComment => "incorrectly-opened-comment",
    InvalidCharacterSequenceAfterDoctypeName => "invalid-character-sequence-after-doctype-name",
    InvalidFirstCharacterOfProcessingInstructionTarget => "invalid-first-character-of-processing-instruction-target",
    InvalidFirstCharacterOfTagName => "invalid-first-character-of-tag-name",
    InvalidProcessingInstructionTarget => "invalid-processing-instruction-target",
    MissingAttributeValue => "missing-attribute-value",
    MissingDoctypeName => "missing-doctype-name",
    MissingDoctypePublicIdentifier => "missing-doctype-public-identifier",
    MissingDoctypeSystemIdentifier => "missing-doctype-system-identifier",
    MissingEndTagName => "missing-end-tag-name",
    MissingQuoteBeforeDoctypePublicIdentifier => "missing-quote-before-doctype-public-identifier",
    MissingQuoteBeforeDoctypeSystemIdentifier => "missing-quote-before-doctype-system-identifier",
    MissingSemicolonAfterCharacterReference => "missing-semicolon-after-character-reference",
    MissingWhitespaceAfterDoctypePublicKeyword => "missing-whitespace-after-doctype-public-keyword",
    MissingWhitespaceAfterDoctypeSystemKeyword => "missing-whitespace-after-doctype-system-keyword",
    MissingWhitespaceBeforeDoctypeName => "missing-whitespace-before-doctype-name",
    MissingWhitespaceBetweenAttributes => "missing-whitespace-between-attributes",
    MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers => "missing-whitespace-between-doctype-public-and-system-identifiers",
    NestedComment => "nested-comment",
    NonVoidHtmlElementStartTagWithTrailingSolidus => "non-void-html-element-start-tag-with-trailing-solidus",
    NoncharacterCharacterReference => "noncharacter-character-reference",
    NoncharacterInInputStream => "noncharacter-in-input-stream",
    NullCharacterReference => "null-character-reference",
    SurrogateCharacterReference => "surrogate-character-reference",
    SurrogateInInputStream => "surrogate-in-input-stream",
    UnexpectedCharacterAfterDoctypeSystemIdentifier => "unexpected-character-after-doctype-system-identifier",
    UnexpectedCharacterInAttributeName => "unexpected-character-in-attribute-name",
    // not one of the standard's: the parser stopped nesting at its depth limit
    TreeTooDeep => "tree-too-deep",
    UnexpectedCharacterInUnquotedAttributeValue => "unexpected-character-in-unquoted-attribute-value",
    UnexpectedEqualsSignBeforeAttributeName => "unexpected-equals-sign-before-attribute-name",
    UnexpectedNullCharacter => "unexpected-null-character",
    UnexpectedSolidusInTag => "unexpected-solidus-in-tag",
    UnknownNamedCharacterReference => "unknown-named-character-reference",
}
