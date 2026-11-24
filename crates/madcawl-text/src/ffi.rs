//! The CoreFoundation and CoreText functions this crate calls, declared by hand.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::c_void;
use std::ptr::NonNull;

pub type CFTypeRef = *const c_void;
pub type CFIndex = isize;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CFRange {
    pub location: CFIndex,
    pub length: CFIndex,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

// the callback structs are only ever passed by address
#[repr(C)]
pub struct Callbacks {
    _private: [u8; 0],
}

pub const kCFNumberSInt32Type: CFIndex = 3;
pub const kCFNumberFloat64Type: CFIndex = 6;
pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;
pub const kCTFontUIFontSystem: u32 = 2;
pub const kCTWritingDirectionRightToLeft: i32 = 1;
pub const kCTWritingDirectionOverride: i32 = 2;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub static kCFTypeDictionaryKeyCallBacks: Callbacks;
    pub static kCFTypeDictionaryValueCallBacks: Callbacks;
    pub static kCFTypeArrayCallBacks: Callbacks;
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    pub fn CFStringCreateWithCharacters(
        alloc: CFTypeRef,
        chars: *const u16,
        len: CFIndex,
    ) -> CFTypeRef;
    pub fn CFStringCreateWithBytes(
        alloc: CFTypeRef,
        bytes: *const u8,
        len: CFIndex,
        enc: u32,
        ext: u8,
    ) -> CFTypeRef;
    pub fn CFStringGetLength(s: CFTypeRef) -> CFIndex;
    pub fn CFStringGetCharacters(s: CFTypeRef, range: CFRange, out: *mut u16);
    pub fn CFDictionaryCreate(
        alloc: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        len: CFIndex,
        key_cb: *const Callbacks,
        value_cb: *const Callbacks,
    ) -> CFTypeRef;
    pub fn CFDictionaryGetValue(d: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    pub fn CFArrayCreate(
        alloc: CFTypeRef,
        values: *const CFTypeRef,
        len: CFIndex,
        cb: *const Callbacks,
    ) -> CFTypeRef;
    pub fn CFArrayGetCount(a: CFTypeRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(a: CFTypeRef, i: CFIndex) -> CFTypeRef;
    pub fn CFNumberCreate(alloc: CFTypeRef, kind: CFIndex, value: *const c_void) -> CFTypeRef;
    pub fn CFAttributedStringCreate(alloc: CFTypeRef, s: CFTypeRef, attrs: CFTypeRef) -> CFTypeRef;
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    pub static kCTFontAttributeName: CFTypeRef;
    pub static kCTLigatureAttributeName: CFTypeRef;
    pub static kCTWritingDirectionAttributeName: CFTypeRef;
    pub static kCTFontFamilyNameAttribute: CFTypeRef;
    pub static kCTFontTraitsAttribute: CFTypeRef;
    pub static kCTFontWeightTrait: CFTypeRef;
    pub static kCTFontSlantTrait: CFTypeRef;
    pub fn CTFontCreateWithName(name: CFTypeRef, size: f64, matrix: *const c_void) -> CFTypeRef;
    pub fn CTFontCreateUIFontForLanguage(kind: u32, size: f64, lang: CFTypeRef) -> CFTypeRef;
    pub fn CTFontCreateCopyWithAttributes(
        font: CFTypeRef,
        size: f64,
        matrix: *const c_void,
        desc: CFTypeRef,
    ) -> CFTypeRef;
    pub fn CTFontCreateForString(font: CFTypeRef, s: CFTypeRef, range: CFRange) -> CFTypeRef;
    pub fn CTFontDescriptorCreateWithAttributes(attrs: CFTypeRef) -> CFTypeRef;
    pub fn CTFontDescriptorCreateMatchingFontDescriptor(
        desc: CFTypeRef,
        mandatory: CFTypeRef,
    ) -> CFTypeRef;
    pub fn CTFontCreateWithFontDescriptor(
        desc: CFTypeRef,
        size: f64,
        matrix: *const c_void,
    ) -> CFTypeRef;
    pub fn CTFontCopyPostScriptName(font: CFTypeRef) -> CFTypeRef;
    pub fn CTFontCopyFamilyName(font: CFTypeRef) -> CFTypeRef;
    pub fn CTFontGetGlyphsForCharacters(
        font: CFTypeRef,
        chars: *const u16,
        glyphs: *mut u16,
        n: CFIndex,
    ) -> bool;
    pub fn CTFontGetSize(font: CFTypeRef) -> f64;
    pub fn CTFontGetAscent(font: CFTypeRef) -> f64;
    pub fn CTFontGetDescent(font: CFTypeRef) -> f64;
    pub fn CTFontGetLeading(font: CFTypeRef) -> f64;
    pub fn CTFontGetXHeight(font: CFTypeRef) -> f64;
    pub fn CTFontGetCapHeight(font: CFTypeRef) -> f64;
    pub fn CTFontGetUnderlinePosition(font: CFTypeRef) -> f64;
    pub fn CTFontGetUnderlineThickness(font: CFTypeRef) -> f64;
    pub fn CTFontGetUnitsPerEm(font: CFTypeRef) -> u32;
    pub fn CTLineCreateWithAttributedString(s: CFTypeRef) -> CFTypeRef;
    pub fn CTLineGetGlyphRuns(line: CFTypeRef) -> CFTypeRef;
    pub fn CTRunGetGlyphCount(run: CFTypeRef) -> CFIndex;
    pub fn CTRunGetGlyphs(run: CFTypeRef, range: CFRange, out: *mut u16);
    pub fn CTRunGetAdvances(run: CFTypeRef, range: CFRange, out: *mut CGSize);
    pub fn CTRunGetPositions(run: CFTypeRef, range: CFRange, out: *mut CGPoint);
    pub fn CTRunGetStringIndices(run: CFTypeRef, range: CFRange, out: *mut CFIndex);
    pub fn CTRunGetAttributes(run: CFTypeRef) -> CFTypeRef;
}

/// An owned CoreFoundation object, released on drop.
pub struct Cf(NonNull<c_void>);

// CoreFoundation retain counts are atomic; only fonts and strings, immutable once created, cross threads
unsafe impl Send for Cf {}
unsafe impl Sync for Cf {}

impl Cf {
    /// Takes ownership of an object returned by a Create or Copy function; `None` for null.
    pub fn new(p: CFTypeRef) -> Option<Cf> {
        NonNull::new(p as *mut c_void).map(Cf)
    }

    pub fn get(&self) -> CFTypeRef {
        self.0.as_ptr()
    }
}

impl Drop for Cf {
    fn drop(&mut self) {
        // SAFETY: the pointer came from a Create or Copy call and is released exactly once
        unsafe { CFRelease(self.get()) }
    }
}

pub fn string(s: &str) -> Option<Cf> {
    // SAFETY: the bytes are valid UTF-8 for the length given
    Cf::new(unsafe {
        CFStringCreateWithBytes(
            std::ptr::null(),
            s.as_ptr(),
            s.len() as CFIndex,
            kCFStringEncodingUTF8,
            0,
        )
    })
}

pub fn utf16(s: &[u16]) -> Option<Cf> {
    // SAFETY: the slice is valid for its length
    Cf::new(unsafe {
        CFStringCreateWithCharacters(std::ptr::null(), s.as_ptr(), s.len() as CFIndex)
    })
}

/// The contents of a CFString as UTF-8.
pub fn to_string(s: &Cf) -> String {
    // SAFETY: `s` is a CFString; the buffer holds exactly the length asked for
    unsafe {
        let n = CFStringGetLength(s.get());
        let mut buf = vec![0u16; n as usize];
        CFStringGetCharacters(
            s.get(),
            CFRange {
                location: 0,
                length: n,
            },
            buf.as_mut_ptr(),
        );
        String::from_utf16_lossy(&buf)
    }
}

pub fn dictionary(keys: &[CFTypeRef], values: &[CFTypeRef]) -> Option<Cf> {
    // SAFETY: both slices have the same length and hold CF objects; the callbacks retain them
    Cf::new(unsafe {
        CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            keys.len().min(values.len()) as CFIndex,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )
    })
}

pub fn number_i32(v: i32) -> Option<Cf> {
    // SAFETY: the value pointer matches the number type
    Cf::new(unsafe {
        CFNumberCreate(
            std::ptr::null(),
            kCFNumberSInt32Type,
            (&v as *const i32).cast(),
        )
    })
}

pub fn number_f64(v: f64) -> Option<Cf> {
    // SAFETY: the value pointer matches the number type
    Cf::new(unsafe {
        CFNumberCreate(
            std::ptr::null(),
            kCFNumberFloat64Type,
            (&v as *const f64).cast(),
        )
    })
}

pub fn array(values: &[CFTypeRef]) -> Option<Cf> {
    // SAFETY: the slice holds CF objects; the callbacks retain them
    Cf::new(unsafe {
        CFArrayCreate(
            std::ptr::null(),
            values.as_ptr(),
            values.len() as CFIndex,
            &kCFTypeArrayCallBacks,
        )
    })
}
