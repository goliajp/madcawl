//! The C interface of the system JavaScriptCore.framework, as much of
//! it as the adapter calls. Declared by hand from the SDK headers.

#![allow(non_camel_case_types, non_upper_case_globals, dead_code)]

use std::ffi::{c_char, c_void};

#[repr(C)]
pub struct OpaqueJSContextGroup(c_void);
#[repr(C)]
pub struct OpaqueJSContext(c_void);
#[repr(C)]
pub struct OpaqueJSString(c_void);
#[repr(C)]
pub struct OpaqueJSClass(c_void);
#[repr(C)]
pub struct OpaqueJSPropertyNameAccumulator(c_void);

pub type JSContextGroupRef = *const OpaqueJSContextGroup;
pub type JSContextRef = *const OpaqueJSContext;
pub type JSGlobalContextRef = *mut OpaqueJSContext;
pub type JSStringRef = *mut OpaqueJSString;
pub type JSClassRef = *mut OpaqueJSClass;
/// A value: on 64-bit targets the engine's encoded value word itself.
pub type JSValueRef = *const c_void;
pub type JSObjectRef = *mut c_void;
pub type JSPropertyNameAccumulatorRef = *mut OpaqueJSPropertyNameAccumulator;
pub type JSChar = u16;

pub type JSType = u32;
pub const kJSTypeUndefined: JSType = 0;
pub const kJSTypeNull: JSType = 1;
pub const kJSTypeBoolean: JSType = 2;
pub const kJSTypeNumber: JSType = 3;
pub const kJSTypeString: JSType = 4;
pub const kJSTypeObject: JSType = 5;
pub const kJSTypeSymbol: JSType = 6;
pub const kJSTypeBigInt: JSType = 7;

pub type JSPropertyAttributes = u32;
pub const kJSPropertyAttributeNone: u32 = 0;
pub const kJSPropertyAttributeReadOnly: u32 = 1 << 1;
pub const kJSPropertyAttributeDontEnum: u32 = 1 << 2;
pub const kJSPropertyAttributeDontDelete: u32 = 1 << 3;

pub type JSClassAttributes = u32;
pub const kJSClassAttributeNoAutomaticPrototype: u32 = 1 << 1;

pub type FinalizeCallback = Option<unsafe extern "C" fn(JSObjectRef)>;
pub type SetPropertyCallback =
    Option<unsafe extern "C" fn(JSContextRef, JSObjectRef, JSStringRef, JSValueRef, *mut JSValueRef) -> bool>;
pub type GetPropertyCallback =
    Option<unsafe extern "C" fn(JSContextRef, JSObjectRef, JSStringRef, *mut JSValueRef) -> JSValueRef>;
pub type CallAsFunctionCallback = Option<
    unsafe extern "C" fn(JSContextRef, JSObjectRef, JSObjectRef, usize, *const JSValueRef, *mut JSValueRef) -> JSValueRef,
>;
pub type HasInstanceCallback = Option<unsafe extern "C" fn(JSContextRef, JSObjectRef, JSValueRef, *mut JSValueRef) -> bool>;
pub type CallAsConstructorCallback =
    Option<unsafe extern "C" fn(JSContextRef, JSObjectRef, usize, *const JSValueRef, *mut JSValueRef) -> JSObjectRef>;

#[repr(C)]
pub struct JSClassDefinition {
    pub version: i32,
    pub attributes: JSClassAttributes,
    pub class_name: *const c_char,
    pub parent_class: JSClassRef,
    pub static_values: *const c_void,
    pub static_functions: *const c_void,
    pub initialize: *const c_void,
    pub finalize: FinalizeCallback,
    pub has_property: *const c_void,
    pub get_property: GetPropertyCallback,
    pub set_property: SetPropertyCallback,
    pub delete_property: *const c_void,
    pub get_property_names: *const c_void,
    pub call_as_function: CallAsFunctionCallback,
    pub call_as_constructor: CallAsConstructorCallback,
    pub has_instance: HasInstanceCallback,
    pub convert_to_type: *const c_void,
}

impl JSClassDefinition {
    pub const EMPTY: JSClassDefinition = JSClassDefinition {
        version: 0,
        attributes: 0,
        class_name: std::ptr::null(),
        parent_class: std::ptr::null_mut(),
        static_values: std::ptr::null(),
        static_functions: std::ptr::null(),
        initialize: std::ptr::null(),
        finalize: None,
        has_property: std::ptr::null(),
        get_property: None,
        set_property: None,
        delete_property: std::ptr::null(),
        get_property_names: std::ptr::null(),
        call_as_function: None,
        call_as_constructor: None,
        has_instance: None,
        convert_to_type: std::ptr::null(),
    };
}

#[link(name = "JavaScriptCore", kind = "framework")]
unsafe extern "C" {
    pub fn JSContextGroupCreate() -> JSContextGroupRef;
    pub fn JSContextGroupRelease(group: JSContextGroupRef);
    pub fn JSGlobalContextCreateInGroup(group: JSContextGroupRef, class: JSClassRef) -> JSGlobalContextRef;
    pub fn JSGlobalContextRelease(ctx: JSGlobalContextRef);
    pub fn JSContextGetGlobalObject(ctx: JSContextRef) -> JSObjectRef;
    pub fn JSGarbageCollect(ctx: JSContextRef);

    pub fn JSEvaluateScript(
        ctx: JSContextRef,
        script: JSStringRef,
        this: JSObjectRef,
        source_url: JSStringRef,
        line: i32,
        exception: *mut JSValueRef,
    ) -> JSValueRef;

    pub fn JSClassCreate(def: *const JSClassDefinition) -> JSClassRef;
    pub fn JSClassRelease(class: JSClassRef);

    pub fn JSObjectMake(ctx: JSContextRef, class: JSClassRef, data: *mut c_void) -> JSObjectRef;
    pub fn JSObjectMakeFunction(
        ctx: JSContextRef,
        name: JSStringRef,
        params: u32,
        names: *const JSStringRef,
        body: JSStringRef,
        source_url: JSStringRef,
        line: i32,
        exception: *mut JSValueRef,
    ) -> JSObjectRef;
    pub fn JSObjectMakeArray(ctx: JSContextRef, argc: usize, argv: *const JSValueRef, exception: *mut JSValueRef)
    -> JSObjectRef;
    pub fn JSObjectMakeError(ctx: JSContextRef, argc: usize, argv: *const JSValueRef, exception: *mut JSValueRef)
    -> JSObjectRef;
    pub fn JSObjectGetPrototype(ctx: JSContextRef, object: JSObjectRef) -> JSValueRef;
    pub fn JSObjectSetPrototype(ctx: JSContextRef, object: JSObjectRef, value: JSValueRef);
    pub fn JSObjectHasProperty(ctx: JSContextRef, object: JSObjectRef, name: JSStringRef) -> bool;
    pub fn JSObjectGetProperty(
        ctx: JSContextRef,
        object: JSObjectRef,
        name: JSStringRef,
        exception: *mut JSValueRef,
    ) -> JSValueRef;
    pub fn JSObjectSetProperty(
        ctx: JSContextRef,
        object: JSObjectRef,
        name: JSStringRef,
        value: JSValueRef,
        attributes: JSPropertyAttributes,
        exception: *mut JSValueRef,
    );
    pub fn JSObjectDeleteProperty(
        ctx: JSContextRef,
        object: JSObjectRef,
        name: JSStringRef,
        exception: *mut JSValueRef,
    ) -> bool;
    pub fn JSObjectGetPropertyAtIndex(
        ctx: JSContextRef,
        object: JSObjectRef,
        index: u32,
        exception: *mut JSValueRef,
    ) -> JSValueRef;
    pub fn JSObjectSetPropertyAtIndex(
        ctx: JSContextRef,
        object: JSObjectRef,
        index: u32,
        value: JSValueRef,
        exception: *mut JSValueRef,
    );
    pub fn JSObjectGetPrivate(object: JSObjectRef) -> *mut c_void;
    pub fn JSObjectIsFunction(ctx: JSContextRef, object: JSObjectRef) -> bool;
    pub fn JSObjectCallAsFunction(
        ctx: JSContextRef,
        object: JSObjectRef,
        this: JSObjectRef,
        argc: usize,
        argv: *const JSValueRef,
        exception: *mut JSValueRef,
    ) -> JSValueRef;

    pub fn JSValueGetType(ctx: JSContextRef, value: JSValueRef) -> JSType;
    pub fn JSValueIsObjectOfClass(ctx: JSContextRef, value: JSValueRef, class: JSClassRef) -> bool;
    pub fn JSValueIsStrictEqual(ctx: JSContextRef, a: JSValueRef, b: JSValueRef) -> bool;
    pub fn JSValueMakeUndefined(ctx: JSContextRef) -> JSValueRef;
    pub fn JSValueMakeNull(ctx: JSContextRef) -> JSValueRef;
    pub fn JSValueMakeBoolean(ctx: JSContextRef, b: bool) -> JSValueRef;
    pub fn JSValueMakeNumber(ctx: JSContextRef, n: f64) -> JSValueRef;
    pub fn JSValueMakeString(ctx: JSContextRef, s: JSStringRef) -> JSValueRef;
    pub fn JSValueToNumber(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> f64;
    pub fn JSValueToBoolean(ctx: JSContextRef, value: JSValueRef) -> bool;
    pub fn JSValueToStringCopy(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> JSStringRef;
    pub fn JSValueToObject(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> JSObjectRef;
    pub fn JSValueProtect(ctx: JSContextRef, value: JSValueRef);
    pub fn JSValueUnprotect(ctx: JSContextRef, value: JSValueRef);

    pub fn JSStringCreateWithCharacters(chars: *const JSChar, len: usize) -> JSStringRef;
    pub fn JSStringCreateWithUTF8CString(s: *const c_char) -> JSStringRef;
    pub fn JSStringRelease(s: JSStringRef);
    pub fn JSStringGetLength(s: JSStringRef) -> usize;
    pub fn JSStringGetCharactersPtr(s: JSStringRef) -> *const JSChar;
}
