use std::{
    fmt,
    sync::{Arc, atomic::{AtomicUsize, Ordering}},
    num::NonZeroU16,
    any::Any,
};
use azul_css::{
    LayoutPoint, LayoutRect, LayoutSize,
    RectStyle, StyleFontSize, ColorU, U8Vec, AzString
};
use crate::{
    FastHashMap, FastHashSet,
    ui_solver::ResolvedTextLayoutOptions,
    display_list::GlyphInstance,
    styled_dom::StyledNode,
    callbacks::PipelineId,
    dom::{NodeData, OptionImageMask},
    svg::{SvgStyledNode, TesselatedCPUSvgNode},
};

#[cfg(feature = "opengl")]
use crate::{gl::Texture, svg::TesselatedGPUSvgNode};

pub type CssImageId = String;
pub type CssFontId = String;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontMetrics {

    // head table

    pub units_per_em: NonZeroU16,
    pub font_flags: u16,
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,

    // hhea table

    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
    pub advance_width_max: u16,
    pub min_left_side_bearing: i16,
    pub min_right_side_bearing: i16,
    pub x_max_extent: i16,
    pub caret_slope_rise: i16,
    pub caret_slope_run: i16,
    pub caret_offset: i16,
    pub num_h_metrics: u16,

    // os/2 table

    pub x_avg_char_width: i16,
    pub us_weight_class: u16,
    pub us_width_class: u16,
    pub fs_type: u16,
    pub y_subscript_x_size: i16,
    pub y_subscript_y_size: i16,
    pub y_subscript_x_offset: i16,
    pub y_subscript_y_offset: i16,
    pub y_superscript_x_size: i16,
    pub y_superscript_y_size: i16,
    pub y_superscript_x_offset: i16,
    pub y_superscript_y_offset: i16,
    pub y_strikeout_size: i16,
    pub y_strikeout_position: i16,
    pub s_family_class: i16,
    pub panose: [u8; 10],
    pub ul_unicode_range1: u32,
    pub ul_unicode_range2: u32,
    pub ul_unicode_range3: u32,
    pub ul_unicode_range4: u32,
    pub ach_vend_id: u32,
    pub fs_selection: u16,
    pub us_first_char_index: u16,
    pub us_last_char_index: u16,

    // os/2 version 0 table

    pub s_typo_ascender: Option<i16>,
    pub s_typo_descender: Option<i16>,
    pub s_typo_line_gap: Option<i16>,
    pub us_win_ascent: Option<u16>,
    pub us_win_descent: Option<u16>,

    // os/2 version 1 table

    pub ul_code_page_range1: Option<u32>,
    pub ul_code_page_range2: Option<u32>,

    // os/2 version 2 table

    pub sx_height: Option<i16>,
    pub s_cap_height: Option<i16>,
    pub us_default_char: Option<u16>,
    pub us_break_char: Option<u16>,
    pub us_max_context: Option<u16>,

    // os/2 version 3 table

    pub us_lower_optical_point_size: Option<u16>,
    pub us_upper_optical_point_size: Option<u16>,
}

impl Default for FontMetrics {
    fn default() -> Self {
        FontMetrics::zero()
    }
}

impl FontMetrics {

    /// Only for testing, zero-sized font, will always return 0 for every metric (`units_per_em = 1000`)
    pub const fn zero() -> Self {
        FontMetrics {
            units_per_em: unsafe { NonZeroU16::new_unchecked(1000) },
            font_flags: 0,
            x_min: 0,
            y_min: 0,
            x_max: 0,
            y_max: 0,
            ascender: 0,
            descender: 0,
            line_gap: 0,
            advance_width_max: 0,
            min_left_side_bearing: 0,
            min_right_side_bearing: 0,
            x_max_extent: 0,
            caret_slope_rise: 0,
            caret_slope_run: 0,
            caret_offset: 0,
            num_h_metrics: 0,
            x_avg_char_width: 0,
            us_weight_class: 0,
            us_width_class: 0,
            fs_type: 0,
            y_subscript_x_size: 0,
            y_subscript_y_size: 0,
            y_subscript_x_offset: 0,
            y_subscript_y_offset: 0,
            y_superscript_x_size: 0,
            y_superscript_y_size: 0,
            y_superscript_x_offset: 0,
            y_superscript_y_offset: 0,
            y_strikeout_size: 0,
            y_strikeout_position: 0,
            s_family_class: 0,
            panose: [0;10],
            ul_unicode_range1: 0,
            ul_unicode_range2: 0,
            ul_unicode_range3: 0,
            ul_unicode_range4: 0,
            ach_vend_id: 0,
            fs_selection: 0,
            us_first_char_index: 0,
            us_last_char_index: 0,
            s_typo_ascender: None,
            s_typo_descender: None,
            s_typo_line_gap: None,
            us_win_ascent: None,
            us_win_descent: None,
            ul_code_page_range1: None,
            ul_code_page_range2: None,
            sx_height: None,
            s_cap_height: None,
            us_default_char: None,
            us_break_char: None,
            us_max_context: None,
            us_lower_optical_point_size: None,
            us_upper_optical_point_size: None,
        }
    }

    /// If set, use `OS/2.sTypoAscender - OS/2.sTypoDescender + OS/2.sTypoLineGap` to calculate the height
    ///
    /// See [`USE_TYPO_METRICS`](https://docs.microsoft.com/en-us/typography/opentype/spec/os2#fss)
    pub fn use_typo_metrics(&self) -> bool {
        self.fs_selection & (1 << 7) != 0
    }

    pub fn get_ascender(&self, target_font_size: f32) -> f32 {
        let use_typo = if !self.use_typo_metrics() { None } else { self.s_typo_ascender };
        if let Some(s) = use_typo {
            s as f32 / self.units_per_em.get() as f32 * target_font_size
        } else {
            self.ascender as f32 / self.units_per_em.get() as f32 * target_font_size
        }
    }

    /// NOTE: descender is NEGATIVE
    pub fn get_descender(&self, target_font_size: f32) -> f32 {
        let use_typo = if !self.use_typo_metrics() { None } else { self.s_typo_descender };
        if let Some(s) = use_typo {
            s as f32 / self.units_per_em.get() as f32 * target_font_size
        } else {
            self.descender as f32 / self.units_per_em.get() as f32 * target_font_size
        }
    }

    pub fn get_line_gap(&self, target_font_size: f32) -> f32 {
        let use_typo = if !self.use_typo_metrics() { None } else { self.s_typo_line_gap };
        if let Some(s) = use_typo {
            s as f32 / self.units_per_em.get() as f32 * target_font_size
        } else {
            self.line_gap as f32 / self.units_per_em.get() as f32 * target_font_size
        }
    }

    /// `height = sTypoAscender - sTypoDescender + sTypoLineGap`
    pub fn get_height(&self, target_font_size: f32) -> f32 {
        self.get_ascender(target_font_size) - self.get_descender(target_font_size) + self.get_line_gap(target_font_size)
    }

    pub fn get_x_min(&self, target_font_size: f32) -> f32 { self.x_min as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_y_min(&self, target_font_size: f32) -> f32 { self.y_min as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_x_max(&self, target_font_size: f32) -> f32 { self.x_max as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_y_max(&self, target_font_size: f32) -> f32 { self.y_max as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_advance_width_max(&self, target_font_size: f32) -> f32 { self.advance_width_max as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_min_left_side_bearing(&self, target_font_size: f32) -> f32 { self.min_left_side_bearing as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_min_right_side_bearing(&self, target_font_size: f32) -> f32 { self.min_right_side_bearing as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_x_max_extent(&self, target_font_size: f32) -> f32 { self.x_max_extent as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_x_avg_char_width(&self, target_font_size: f32) -> f32 { self.x_avg_char_width as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_y_subscript_x_size(&self, target_font_size: f32) -> f32 { self.y_subscript_x_size as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_y_subscript_y_size(&self, target_font_size: f32) -> f32 { self.y_subscript_y_size as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_y_subscript_x_offset(&self, target_font_size: f32) -> f32 { self.y_subscript_x_offset as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_y_subscript_y_offset(&self, target_font_size: f32) -> f32 { self.y_subscript_y_offset as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_y_superscript_x_size(&self, target_font_size: f32) -> f32 { self.y_superscript_x_size as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_y_superscript_y_size(&self, target_font_size: f32) -> f32 { self.y_superscript_y_size as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_y_superscript_x_offset(&self, target_font_size: f32) -> f32 { self.y_superscript_x_offset as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_y_superscript_y_offset(&self, target_font_size: f32) -> f32 { self.y_superscript_y_offset as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_y_strikeout_size(&self, target_font_size: f32) -> f32 { self.y_strikeout_size as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_y_strikeout_position(&self, target_font_size: f32) -> f32 { self.y_strikeout_position as f32 / self.units_per_em.get() as f32 * target_font_size }
    pub fn get_s_typo_ascender(&self, target_font_size: f32) -> Option<f32> { self.s_typo_ascender.map(|s| s as f32 / self.units_per_em.get() as f32 * target_font_size) }
    pub fn get_s_typo_descender(&self, target_font_size: f32) -> Option<f32> { self.s_typo_descender.map(|s| s as f32 / self.units_per_em.get() as f32 * target_font_size) }
    pub fn get_s_typo_line_gap(&self, target_font_size: f32) -> Option<f32> { self.s_typo_line_gap.map(|s| s as f32 / self.units_per_em.get() as f32 * target_font_size) }
    pub fn get_us_win_ascent(&self, target_font_size: f32) -> Option<f32> { self.us_win_ascent.map(|s| s as f32 / self.units_per_em.get() as f32 * target_font_size) }
    pub fn get_us_win_descent(&self, target_font_size: f32) -> Option<f32> { self.us_win_descent.map(|s| s as f32 / self.units_per_em.get() as f32 * target_font_size) }
    pub fn get_sx_height(&self, target_font_size: f32) -> Option<f32> { self.sx_height.map(|s| s as f32 / self.units_per_em.get() as f32 * target_font_size) }
    pub fn get_s_cap_height(&self, target_font_size: f32) -> Option<f32> { self.s_cap_height.map(|s| s as f32 / self.units_per_em.get() as f32 * target_font_size) }
}

pub type WordIndex = usize;
pub type GlyphIndex = usize;
pub type LineLength = f32;
pub type IndexOfLineBreak = usize;
pub type RemainingSpaceToRight = f32;
pub type LineBreaks = Vec<(GlyphIndex, RemainingSpaceToRight)>;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrimitiveFlags {
    /// The CSS backface-visibility property (yes, it can be really granular)
    pub is_backface_visible: bool,
    /// If set, this primitive represents a scroll bar container
    pub is_scrollbar_container: bool,
    /// If set, this primitive represents a scroll bar thumb
    pub is_scrollbar_thumb: bool,
}

/// Metadata (but not storage) describing an image In WebRender.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImageDescriptor {
    /// Format of the image data.
    pub format: RawImageFormat,
    /// Width and height of the image data, in pixels.
    pub dimensions: (usize, usize),
    /// The number of bytes from the start of one row to the next. If non-None,
    /// `compute_stride` will return this value, otherwise it returns
    /// `width * bpp`. Different source of images have different alignment
    /// constraints for rows, so the stride isn't always equal to width * bpp.
    pub stride: Option<i32>,
    /// Offset in bytes of the first pixel of this image in its backing buffer.
    /// This is used for tiling, wherein WebRender extracts chunks of input images
    /// in order to cache, manipulate, and render them individually. This offset
    /// tells the texture upload machinery where to find the bytes to upload for
    /// this tile. Non-tiled images generally set this to zero.
    pub offset: i32,
    /// Various bool flags related to this descriptor.
    pub flags: ImageDescriptorFlags,
}

/// Various flags that are part of an image descriptor.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImageDescriptorFlags {
    /// Whether this image is opaque, or has an alpha channel. Avoiding blending
    /// for opaque surfaces is an important optimization.
    pub is_opaque: bool,
    /// Whether to allow the driver to automatically generate mipmaps. If images
    /// are already downscaled appropriately, mipmap generation can be wasted
    /// work, and cause performance problems on some cards/drivers.
    ///
    /// See https://github.com/servo/webrender/pull/2555/
    pub allow_mipmaps: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdNamespace(pub u32);

impl ::std::fmt::Display for IdNamespace {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "IdNamespace({})", self.0)
    }
}

impl ::std::fmt::Debug for IdNamespace {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub enum RawImageFormat {
    R8,
    R16,
    RG16,
    BGRA8,
    RGBAF32,
    RG8,
    RGBAI32,
    RGBA8,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImageKey {
    pub namespace: IdNamespace,
    pub key: u32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontInstanceKey {
    pub namespace: IdNamespace,
    pub key: u32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontKey {
    pub namespace: IdNamespace,
    pub key: u32,
}

/// Stores the resources for the application, souch as fonts, images and cached
/// texts, also clipboard strings
///
/// Images and fonts can be references across window contexts (not yet tested,
/// but should work).
#[derive(Debug, Default)]
pub struct AppResources {
    /// The CssImageId is the string used in the CSS, i.e. "my_image" -> ImageId(4)
    pub css_ids_to_image_ids: FastHashMap<CssImageId, ImageId>,
    /// Same as CssImageId -> ImageId, but for fonts, i.e. "Roboto" -> FontId(9)
    pub css_ids_to_font_ids: FastHashMap<CssFontId, FontId>,
    /// Stores where the images were loaded from
    pub image_sources: FastHashMap<ImageId, ImageSource>,
    /// Stores where the fonts were loaded from
    pub font_sources: FastHashMap<FontId, FontSource>,
    /// All image keys currently active in the RenderApi
    pub currently_registered_images: FastHashMap<PipelineId, FastHashMap<ImageId, ImageInfo>>,
    /// All font keys currently active in the RenderApi
    pub currently_registered_fonts: FastHashMap<PipelineId, FastHashMap<ImmediateFontId, LoadedFont>>,
    /// If an image isn't displayed, it is deleted from memory, only
    /// the `ImageSource` (i.e. the path / source where the image was loaded from) remains.
    ///
    /// This way the image can be re-loaded if necessary but doesn't have to reside in memory at all times.
    pub last_frame_image_keys: FastHashMap<PipelineId, FastHashSet<ImageId>>,
    /// If a font does not get used for one frame, the corresponding instance key gets
    /// deleted. If a FontId has no FontInstanceKeys anymore, the font key gets deleted.
    ///
    /// The only thing remaining in memory permanently is the FontSource (which is only
    /// the string of the file path where the font was loaded from, so no huge memory pressure).
    /// The reason for this agressive strategy is that the
    pub last_frame_font_keys: FastHashMap<PipelineId, FastHashMap<ImmediateFontId, FastHashSet<Au>>>,
    /// Stores long texts across frames
    pub text_cache: TextCache,
    /// Cache from SVG node ID -> Geometry for that node
    pub svg_cpu_path_cache: FastHashMap<SvgNodeId, SvgStyledNode>,
    /// Cache from SVG node -> CPU tesselated triangles
    pub svg_cpu_tess_cache: FastHashMap<SvgNodeId, TesselatedCPUSvgNode>,
    /// Cache from SVG node -> GPU vertex buffer ID + GPU index buffer ID
    #[cfg(feature = "opengl")]
    pub svg_gpu_tess_cache: FastHashMap<SvgNodeId, TesselatedGPUSvgNode>,
    /// Cache from SVG node -> Texture
    #[cfg(feature = "opengl")]
    pub svg_gpu_texture_cache: FastHashMap<SvgNodeId, Texture>,
}

impl AppResources {

    /// Add a new pipeline to the app resources
    pub fn add_pipeline(&mut self, pipeline_id: PipelineId) {
        self.currently_registered_fonts.insert(pipeline_id, FastHashMap::default());
        self.currently_registered_images.insert(pipeline_id, FastHashMap::default());
        self.last_frame_font_keys.insert(pipeline_id, FastHashMap::default());
        self.last_frame_image_keys.insert(pipeline_id, FastHashSet::default());
    }

    /// Delete and remove all fonts & font instance keys from a given pipeline
    pub fn delete_pipeline<T: FontImageApi>(&mut self, pipeline_id: &PipelineId, render_api: &mut T) {
        let mut delete_font_resources = Vec::new();

        for (font_id, loaded_font) in self.currently_registered_fonts[&pipeline_id].iter() {
            delete_font_resources.extend(
                loaded_font.font_instances.iter()
                .map(|(au, font_instance_key)| (font_id.clone(), DeleteFontMsg::Instance(*font_instance_key, *au)))
            );
            delete_font_resources.push((font_id.clone(), DeleteFontMsg::Font(loaded_font.font_key)));
        }

        let delete_image_resources = self.currently_registered_images[&pipeline_id].iter()
        .map(|(id, info)| (*id, DeleteImageMsg(info.key, *info)))
        .collect();

        delete_resources(self, render_api, pipeline_id, delete_font_resources, delete_image_resources);

        self.currently_registered_fonts.remove(pipeline_id);
        self.currently_registered_images.remove(pipeline_id);
        self.last_frame_font_keys.remove(pipeline_id);
        self.last_frame_image_keys.remove(pipeline_id);
    }
}

macro_rules! unique_id {($struct_name:ident, $counter_name:ident) => {

    static $counter_name: ::std::sync::atomic::AtomicUsize = ::std::sync::atomic::AtomicUsize::new(0);

    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
    #[repr(C)]
    pub struct $struct_name {
        id: usize,
    }

    impl $struct_name {

        pub fn new() -> Self {
            Self { id: $counter_name.fetch_add(1, ::std::sync::atomic::Ordering::SeqCst) }
        }
    }
}}

unique_id!(SvgNodeId, SVG_NODE_ID_COUNTER);
unique_id!(TextId, TEXT_ID_COUNTER);
unique_id!(ImageId, IMAGE_ID_COUNTER);
unique_id!(FontId, FONT_ID_COUNTER);

#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub enum ImageSource {
    /// The image is embedded inside the binary file
    Embedded(U8Vec),
    /// The image is already decoded and loaded from a set of bytes
    Raw(RawImage),
    /// The image is loaded from a file
    File(AzString),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub enum FontSource {
    /// The font is embedded inside the binary file
    Embedded(U8Vec),
    /// The font is loaded from a file
    File(AzString),
    /// The font is a system built-in font
    System(AzString),
}

impl fmt::Display for FontSource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::FontSource::*;
        match self {
            Embedded(e) => write!(f, "Embedded(0x{:x})", e.ptr_as_usize()),
            File(p) => write!(f, "\"{}\"", p),
            System(id) => write!(f, "\"{}\"", id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImmediateFontId {
    Resolved(FontId),
    Unresolved(CssFontId),
}

/// Raw image made up of raw pixels (either BRGA8 or A8)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub struct RawImage {
    pub pixels: U8Vec,
    pub width: usize,
    pub height: usize,
    pub data_format: RawImageFormat,
}

impl RawImage {
    pub fn null_image() -> Self {
        Self {
            pixels: Vec::new().into(),
            width: 0,
            height: 0,
            data_format: RawImageFormat::RGBA8
        }
    }
}

impl_option!(RawImage, OptionRawImage, copy = false, [Debug, Clone, PartialEq, Eq]);

pub struct LoadedFont {
    pub font_key: FontKey,
    // NOTE(fschutt): This is ugly and a hack, but currently I'm too lazy
    // to do it properly: azul-core should not depend on any crate,
    // but the LoadedFont should store the parsed font tables (so that parsing
    // the font is cached and has to be done once).
    //
    // The proper way would be to copy + paste all data structures from allsorts
    // and azul-text-layout, but the improper way is to store it as a Box<Any>
    // and just upcast / downcast it
    pub font: Box<dyn Any>, // = Box<azul_text_layout::Font>
    pub font_instances: FastHashMap<Au, FontInstanceKey>,
    pub font_metrics: FontMetrics,
}

impl fmt::Debug for LoadedFont {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "LoadedFont {{ font_key: {:?}, font_instances: {:#?} }}", self.font_key, self.font_instances)
    }
}

impl LoadedFont {

    pub fn delete_font_instance(&mut self, size: &Au) {
        self.font_instances.remove(size);
    }
}

/// Cache for accessing large amounts of text
#[derive(Debug, Default, Clone)]
pub struct TextCache {
    /// Mapping from the TextID to the actual, UTF-8 String
    ///
    /// This is stored outside of the actual glyph calculation, because usually you don't
    /// need the string, except for rebuilding a cached string (for example, when the font is changed)
    pub string_cache: FastHashMap<TextId, Words>,

    // -- for now, don't cache ScaledWords, it's too complicated...

    // /// Caches the layout of the strings / words.
    // ///
    // /// TextId -> FontId (to look up by font)
    // /// FontId -> PixelValue (to categorize by size within a font)
    // /// PixelValue -> layouted words (to cache the glyph widths on a per-font-size basis)
    // pub(crate) layouted_strings_cache: FastHashMap<TextId, FastHashMap<FontInstanceKey, ScaledWords>>,
}

impl TextCache {

    /// Add a new, large text to the resources
    pub fn add_text(&mut self, words: Words) -> TextId {
        let id = TextId::new();
        self.string_cache.insert(id, words);
        id
    }

    pub fn get_text(&self, text_id: &TextId) -> Option<&Words> {
        self.string_cache.get(text_id)
    }

    /// Removes a string from the string cache, but not the layouted text cache
    pub fn delete_text(&mut self, id: TextId) {
        self.string_cache.remove(&id);
    }

    pub fn clear_all_texts(&mut self) {
        self.string_cache.clear();
    }
}

/// Text broken up into `Tab`, `Word()`, `Return` characters
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Words {
    /// Words (and spaces), broken up into semantic items
    pub items: Vec<Word>,
    /// String that makes up this paragraph of words
    pub internal_str: String,
    /// `internal_chars` is used in order to enable copy-paste (since taking a sub-string isn't possible using UTF-8)
    pub internal_chars: Vec<char>,
}

impl Words {

    pub fn get_substr(&self, word: &Word) -> String {
        self.internal_chars[word.start..word.end].iter().collect()
    }

    pub fn get_str(&self) -> &str {
        &self.internal_str
    }

    pub fn get_char(&self, idx: usize) -> Option<char> {
        self.internal_chars.get(idx).cloned()
    }
}

/// Section of a certain type
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Word {
    pub start: usize,
    pub end: usize,
    pub word_type: WordType,
}

/// Either a white-space delimited word, tab or return character
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WordType {
    /// Encountered a word (delimited by spaces)
    Word,
    // `\t` or `x09`
    Tab,
    /// `\r`, `\n` or `\r\n`, escaped: `\x0D`, `\x0A` or `\x0D\x0A`
    Return,
    /// Space character
    Space,
}

/// A paragraph of words that are shaped and scaled (* but not yet layouted / positioned*!)
/// according to their final size in pixels.
#[derive(Debug, Clone)]
pub struct ShapedWords {
    /// Words scaled to their appropriate font size, but not yet positioned on the screen
    pub items: Vec<ShapedWord>,
    /// Longest word in the `self.scaled_words`, necessary for
    /// calculating overflow rectangles.
    pub longest_word_width: usize,
    /// Horizontal advance of the space glyph
    pub space_advance: usize,
    /// Metrics necessary for baseline calculation
    pub font_metrics: FontMetrics,
}

impl ShapedWords {
    pub fn get_longest_word_width_px(&self, target_font_size: f32) -> f32 {
        self.longest_word_width as f32 / self.font_metrics.units_per_em.get() as f32 * target_font_size
    }
    pub fn get_space_advance_px(&self, target_font_size: f32) -> f32 {
        self.space_advance as f32 / self.font_metrics.units_per_em.get() as f32 * target_font_size
    }
    /// Get the distance from the top of the text to the baseline of the text (= ascender)
    pub fn get_baseline_px(&self, target_font_size: f32) -> f32 {
        target_font_size + self.font_metrics.get_descender(target_font_size)
    }
}

/// A Unicode variation selector.
///
/// VS04-VS14 are omitted as they aren't currently used.
#[derive(Debug, Copy, PartialEq, PartialOrd, Clone, Hash)]
pub enum VariationSelector {
    /// VARIATION SELECTOR-1
    VS01 = 1,
    /// VARIATION SELECTOR-2
    VS02 = 2,
    /// VARIATION SELECTOR-3
    VS03 = 3,
    /// Text presentation
    VS15 = 15,
    /// Emoji presentation
    VS16 = 16,
}

#[derive(Debug, Copy, PartialEq, PartialOrd, Clone, Hash)]
pub enum GlyphOrigin {
    Char(char),
    Direct,
}

#[derive(Debug, Copy, PartialEq, PartialOrd, Clone, Hash)]
pub enum Placement {
    None,
    Distance(i32, i32),
    Anchor(Anchor, Anchor),
}

impl Placement {
    #[inline]
    pub fn get_placement_relative(&self, font_metrics: &FontMetrics, target_font_size: f32) -> (f32, f32) {
        let font_metrics_divisor = font_metrics.units_per_em.get() as f32 / target_font_size;
        match self {
            Placement::None | Placement::Anchor(_, _) => (0.0, 0.0),
            Placement::Distance(x, y) => (*x as f32 / font_metrics_divisor, *y as f32 / font_metrics_divisor),
        }
    }
}

#[derive(Debug, Copy, PartialEq, PartialOrd, Clone, Hash)]
pub enum MarkPlacement {
    None,
    MarkAnchor(usize, Anchor, Anchor),
    MarkOverprint(usize),
}

impl MarkPlacement {
    #[inline]
    pub fn get_placement_relative(&self, font_metrics: &FontMetrics, target_font_size: f32) -> (f32, f32) {
        match self {
            MarkPlacement::None => (0.0, 0.0),
            MarkPlacement::MarkAnchor(_, a, _) => {
                let font_metrics_divisor = font_metrics.units_per_em.get() as f32 / target_font_size;
                (a.x as f32 / font_metrics_divisor, a.y as f32 / font_metrics_divisor)
            },
            MarkPlacement::MarkOverprint(_) => (0.0, 0.0),
        }
    }
}

#[derive(Debug, Copy, PartialEq, PartialOrd, Clone, Hash)]
pub struct Anchor {
    pub x: i16,
    pub y: i16,
}

#[derive(Debug, Copy, PartialEq, PartialOrd, Clone, Hash)]
pub struct RawGlyph {
    pub unicodes: [char; 1],
    pub glyph_index: u16,
    pub liga_component_pos: u16,
    pub glyph_origin: GlyphOrigin,
    pub small_caps: bool,
    pub multi_subst_dup: bool,
    pub is_vert_alt: bool,
    pub fake_bold: bool,
    pub fake_italic: bool,
    pub variation: Option<VariationSelector>,
    pub extra_data: (),
}

#[derive(Debug, Copy, PartialEq, PartialOrd, Clone, Hash)]
pub struct Info {
    pub glyph: RawGlyph,
    pub size: Advance,
    pub placement: Placement,
    pub mark_placement: MarkPlacement,
}

#[derive(Debug, Default, Copy, PartialEq, PartialOrd, Clone, Hash)]
pub struct Advance {
    pub advance_x: u16,
    pub size_x: i32,
    pub size_y: i32,
    pub kerning: i16,
}

impl Advance {

    #[inline]
    pub fn get_x_advance_total_unscaled(&self) -> i32 { self.advance_x as i32 + self.kerning as i32 }
    #[inline]
    pub fn get_x_advance_unscaled(&self) -> i32 { self.advance_x as i32 }
    #[inline]
    pub fn get_x_size_unscaled(&self) -> i32 { self.size_x as i32 }
    #[inline]
    pub fn get_y_size_unscaled(&self) -> i32 { self.size_y as i32 }
    #[inline]
    pub fn get_kerning_unscaled(&self) -> i32 { self.kerning as i32 }

    #[inline]
    pub fn get_x_advance_total_scaled(&self, font_metrics: &FontMetrics, target_font_size: f32) -> f32 {
        self.get_x_advance_total_unscaled() as f32 / font_metrics.units_per_em.get() as f32 * target_font_size
    }
    #[inline]
    pub fn get_x_advance_scaled(&self, font_metrics: &FontMetrics, target_font_size: f32) -> f32 {
        self.get_x_advance_unscaled() as f32 / font_metrics.units_per_em.get() as f32 * target_font_size
    }
    #[inline]
    pub fn get_x_size_scaled(&self, font_metrics: &FontMetrics, target_font_size: f32) -> f32 {
        self.get_x_size_unscaled() as f32 / font_metrics.units_per_em.get() as f32 * target_font_size
    }
    #[inline]
    pub fn get_y_size_scaled(&self, font_metrics: &FontMetrics, target_font_size: f32) -> f32 {
        self.get_y_size_unscaled() as f32 / font_metrics.units_per_em.get() as f32 * target_font_size
    }
    #[inline]
    pub fn get_kerning_scaled(&self, font_metrics: &FontMetrics, target_font_size: f32) -> f32 {
        self.get_kerning_unscaled() as f32 / font_metrics.units_per_em.get() as f32 * target_font_size
    }
}

/// Word that is scaled (to a font / font instance), but not yet positioned
#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub struct ShapedWord {
    /// Glyph codepoint, glyph ID + kerning data
    pub glyph_infos: Vec<Info>,
    /// The sum of the width of all the characters in this word
    pub word_width: usize,
}

impl ShapedWord {
    pub fn get_word_width(&self, font_metrics: &FontMetrics, target_font_size: f32) -> f32 {
        self.word_width as f32 / font_metrics.units_per_em.get() as f32 * target_font_size
    }
    /// Returns the number of glyphs THAT ARE NOT DIACRITIC MARKS
    pub fn number_of_glyphs(&self) -> usize {
        self.glyph_infos.iter().filter(|i| i.mark_placement == MarkPlacement::None).count()
    }
}

/// Stores the positions of the vertically laid out texts
#[derive(Debug, Clone, PartialEq)]
pub struct WordPositions {
    /// Options like word spacing, character spacing, etc. that were
    /// used to layout these glyphs
    pub text_layout_options: ResolvedTextLayoutOptions,
    /// Stores the positions of words.
    pub word_positions: Vec<LayoutPoint>,
    /// Index of the word at which the line breaks + length of line
    /// (useful for text selection + horizontal centering)
    pub line_breaks: Vec<(WordIndex, LineLength)>,
    /// Horizontal width of the last line (in pixels), necessary for inline layout later on,
    /// so that the next text run can contine where the last text run left off.
    ///
    /// Usually, the "trailing" of the current text block is the "leading" of the
    /// next text block, to make it seem like two text runs push into each other.
    pub trailing: f32,
    /// How many words are in the text?
    pub number_of_words: usize,
    /// How many lines (NOTE: virtual lines, meaning line breaks in the layouted text) are there?
    pub number_of_lines: usize,
    /// Horizontal and vertical boundaries of the layouted words.
    ///
    /// Note that the vertical extent can be larger than the last words' position,
    /// because of trailing negative glyph advances.
    pub content_size: LayoutSize,
}

/// Returns the layouted glyph instances
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutedGlyphs {
    pub glyphs: Vec<GlyphInstance>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImageInfo {
    pub key: ImageKey,
    pub descriptor: ImageDescriptor,
}

impl ImageInfo {
    /// Returns the (width, height) of this image.
    pub fn get_dimensions(&self) -> (usize, usize) {
        self.descriptor.dimensions
    }
}

impl AppResources {

    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the IDs of all currently loaded fonts in `self.font_data`
    pub fn get_loaded_font_ids(&self) -> Vec<FontId> {
        self.font_sources.keys().cloned().collect()
    }

    pub fn get_loaded_image_ids(&self) -> Vec<ImageId> {
        self.image_sources.keys().cloned().collect()
    }

    pub fn get_loaded_css_image_ids(&self) -> Vec<CssImageId> {
        self.css_ids_to_image_ids.keys().cloned().collect()
    }

    pub fn get_loaded_css_font_ids(&self) -> Vec<CssFontId> {
        self.css_ids_to_font_ids.keys().cloned().collect()
    }

    pub fn get_loaded_text_ids(&self) -> Vec<TextId> {
        self.text_cache.string_cache.keys().cloned().collect()
    }

    // -- ImageId cache

    /// Add an image from a PNG, JPEG or other source.
    ///
    /// Note: For specialized image formats, you'll have to enable them as
    /// features in the Cargo.toml file.
    pub fn add_image_source(&mut self, image_id: ImageId, image_source: ImageSource) {
        self.image_sources.insert(image_id, image_source);
    }

    /// Returns whether the AppResources has currently a certain image ID registered
    pub fn has_image_source(&self, image_id: &ImageId) -> bool {
        self.image_sources.get(image_id).is_some()
    }

    /// Given an `ImageId`, returns the decoded bytes of that image or `None`, if the `ImageId` is invalid.
    /// Returns an error on IO failure / image decoding failure or image
    pub fn get_image_source(&self, image_id: &ImageId) -> Option<&ImageSource> {
        self.image_sources.get(image_id)
    }

    pub fn delete_image_source(&mut self, image_id: &ImageId) {
        self.image_sources.remove(image_id);
    }

    pub fn add_css_image_id<S: Into<String>>(&mut self, css_id: S) -> ImageId {
        *self.css_ids_to_image_ids.entry(css_id.into()).or_insert_with(|| ImageId::new())
    }

    pub fn has_css_image_id(&self, css_id: &str) -> bool {
        self.get_css_image_id(css_id).is_some()
    }

    pub fn get_css_image_id(&self, css_id: &str) -> Option<&ImageId> {
        self.css_ids_to_image_ids.get(css_id)
    }

    pub fn delete_css_image_id(&mut self, css_id: &str) -> Option<ImageId> {
        self.css_ids_to_image_ids.remove(css_id)
    }

    pub fn get_image_info(&self, pipeline_id: &PipelineId, image_key: &ImageId) -> Option<&ImageInfo> {
        self.currently_registered_images.get(pipeline_id).and_then(|map| map.get(image_key))
    }

    // -- FontId cache

    pub fn add_css_font_id<S: Into<String>>(&mut self, css_id: S) -> FontId {
        *self.css_ids_to_font_ids.entry(css_id.into()).or_insert_with(|| FontId::new())
    }

    pub fn has_css_font_id(&self, css_id: &str) -> bool {
        self.get_css_font_id(css_id).is_some()
    }

    pub fn get_css_font_id(&self, css_id: &str) -> Option<&FontId> {
        self.css_ids_to_font_ids.get(css_id)
    }

    pub fn delete_css_font_id(&mut self, css_id: &str) -> Option<FontId> {
        self.css_ids_to_font_ids.remove(css_id)
    }

    pub fn add_font_source(&mut self, font_id: FontId, font_source: FontSource) {
        self.font_sources.insert(font_id, font_source);
    }

    /// Given a `FontId`, returns the bytes for that font or `None`, if the `FontId` is invalid.
    pub fn get_font_source(&self, font_id: &FontId) -> Option<&FontSource> {
        self.font_sources.get(font_id)
    }

    /// Checks if a `FontId` is valid, i.e. if a font is currently ready-to-use
    pub fn has_font_source(&self, id: &FontId) -> bool {
        self.font_sources.get(id).is_some()
    }

    pub fn delete_font_source(&mut self, id: &FontId) {
        self.font_sources.remove(id);
    }

    pub fn get_loaded_font(&self, pipeline_id: &PipelineId, font_id: &ImmediateFontId) -> Option<&LoadedFont> {
        self.currently_registered_fonts.get(pipeline_id).and_then(|map| map.get(font_id))
    }

    pub fn get_loaded_font_mut(&mut self, pipeline_id: &PipelineId, font_id: &ImmediateFontId) -> Option<&mut LoadedFont> {
        self.currently_registered_fonts.get_mut(pipeline_id).and_then(|map| map.get_mut(font_id))
    }

    // -- TextId cache

    /// Adds a string to the internal text cache, but only store it as a string,
    /// without caching the layout of the string.
    pub fn add_text(&mut self, words: Words) -> TextId {
        self.text_cache.add_text(words)
    }

    pub fn get_text(&self, id: &TextId) -> Option<&Words> {
        self.text_cache.get_text(id)
    }

    /// Removes a string from both the string cache and the layouted text cache
    pub fn delete_text(&mut self, id: TextId) {
        self.text_cache.delete_text(id);
    }

    /// Empties the entire internal text cache, invalidating all `TextId`s. Use with care.
    pub fn clear_all_texts(&mut self) {
        self.text_cache.clear_all_texts();
    }
}

pub trait FontImageApi {
    fn new_image_key(&self) -> ImageKey;
    fn new_font_key(&self) -> FontKey;
    fn new_font_instance_key(&self) -> FontInstanceKey;
    fn update_resources(&self, _: Vec<ResourceUpdate>);
    fn flush_scene_builder(&self);
}

/// Used only for debugging, so that the AppResource garbage
/// collection tests can run without a real RenderApi
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct FakeRenderApi { }

impl FakeRenderApi { pub fn new() -> Self { Self { } } }

static LAST_FAKE_IMAGE_KEY: AtomicUsize = AtomicUsize::new(0);
static LAST_FAKE_FONT_KEY: AtomicUsize = AtomicUsize::new(0);
static LAST_FAKE_FONT_INSTANCE_KEY: AtomicUsize = AtomicUsize::new(0);

// Fake RenderApi for unit testing
impl FontImageApi for FakeRenderApi {
    fn new_image_key(&self) -> ImageKey { ImageKey { key: LAST_FAKE_IMAGE_KEY.fetch_add(1, Ordering::SeqCst) as u32, namespace: IdNamespace(0) } }
    fn new_font_key(&self) -> FontKey { FontKey { key: LAST_FAKE_FONT_KEY.fetch_add(1, Ordering::SeqCst) as u32, namespace: IdNamespace(0) } }
    fn new_font_instance_key(&self) -> FontInstanceKey { FontInstanceKey { key: LAST_FAKE_FONT_INSTANCE_KEY.fetch_add(1, Ordering::SeqCst) as u32, namespace: IdNamespace(0) } }
    fn update_resources(&self, _: Vec<ResourceUpdate>) { }
    fn flush_scene_builder(&self) { }
}

/// Scans the DisplayList for new images and fonts. After this call, the RenderApi is
/// guaranteed to know about all FontKeys and FontInstanceKey
pub fn add_fonts_and_images<U: FontImageApi>(
    app_resources: &mut AppResources,
    render_api: &mut U,
    pipeline_id: &PipelineId,
    styled_nodes: &[StyledNode],
    node_data: &[NodeData],
    load_font_fn: LoadFontFn,
    load_image_fn: LoadImageFn,
    parse_font_fn: ParseFontFn,
) {
    let font_keys = scan_styled_nodes_for_font_keys(&app_resources, styled_nodes, node_data);
    let image_keys = scan_styled_nodes_for_image_keys(&app_resources, styled_nodes, node_data);

    app_resources.last_frame_font_keys.get_mut(pipeline_id).unwrap().extend(font_keys.clone().into_iter());
    app_resources.last_frame_image_keys.get_mut(pipeline_id).unwrap().extend(image_keys.clone().into_iter());

    let add_font_resource_updates = build_add_font_resource_updates(app_resources, render_api, pipeline_id, &font_keys, load_font_fn, parse_font_fn);
    let add_image_resource_updates = build_add_image_resource_updates(app_resources, render_api, pipeline_id, &image_keys, load_image_fn);

    add_resources(app_resources, render_api, pipeline_id, add_font_resource_updates, add_image_resource_updates);
}

/// To be called at the end of a frame (after the UI has rendered):
/// Deletes all FontKeys and FontImageKeys that weren't used in
/// the last frame, to save on memory. If the font needs to be recreated, it
/// needs to be reloaded from the `FontSource`.
pub fn garbage_collect_fonts_and_images<U: FontImageApi>(
    app_resources: &mut AppResources,
    render_api: &mut U,
    pipeline_id: &PipelineId,
) {
    let delete_font_resource_updates = build_delete_font_resource_updates(app_resources, pipeline_id);
    let delete_image_resource_updates = build_delete_image_resource_updates(app_resources, pipeline_id);

    delete_resources(app_resources, render_api, pipeline_id, delete_font_resource_updates, delete_image_resource_updates);

    app_resources.last_frame_font_keys.get_mut(pipeline_id).unwrap().clear();
    app_resources.last_frame_image_keys.get_mut(pipeline_id).unwrap().clear();
}

pub fn font_size_to_au(font_size: StyleFontSize) -> Au {
    use crate::ui_solver::DEFAULT_FONT_SIZE_PX;
    Au::from_px(font_size.inner.to_pixels(DEFAULT_FONT_SIZE_PX as f32))
}

pub type FontInstanceFlags = u32;

// Common flags
pub const FONT_INSTANCE_FLAG_SYNTHETIC_BOLD: u32    = 1 << 1;
pub const FONT_INSTANCE_FLAG_EMBEDDED_BITMAPS: u32  = 1 << 2;
pub const FONT_INSTANCE_FLAG_SUBPIXEL_BGR: u32      = 1 << 3;
pub const FONT_INSTANCE_FLAG_TRANSPOSE: u32         = 1 << 4;
pub const FONT_INSTANCE_FLAG_FLIP_X: u32            = 1 << 5;
pub const FONT_INSTANCE_FLAG_FLIP_Y: u32            = 1 << 6;
pub const FONT_INSTANCE_FLAG_SUBPIXEL_POSITION: u32 = 1 << 7;

// Windows flags
pub const FONT_INSTANCE_FLAG_FORCE_GDI: u32         = 1 << 16;

// Mac flags
pub const FONT_INSTANCE_FLAG_FONT_SMOOTHING: u32    = 1 << 16;

// FreeType flags
pub const FONT_INSTANCE_FLAG_FORCE_AUTOHINT: u32    = 1 << 16;
pub const FONT_INSTANCE_FLAG_NO_AUTOHINT: u32       = 1 << 17;
pub const FONT_INSTANCE_FLAG_VERTICAL_LAYOUT: u32   = 1 << 18;
pub const FONT_INSTANCE_FLAG_LCD_VERTICAL: u32      = 1 << 19;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct GlyphOptions {
    pub render_mode: FontRenderMode,
    pub flags: FontInstanceFlags,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub enum FontRenderMode {
    Mono,
    Alpha,
    Subpixel,
}

#[cfg(target_arch = "wasm32")]
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct FontInstancePlatformOptions {
    // empty for now
}

#[cfg(target_os = "windows")]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct FontInstancePlatformOptions {
    pub gamma: u16,
    pub contrast: u8,
    pub cleartype_level: u8,
}

#[cfg(target_os = "macos")]
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct FontInstancePlatformOptions {
    pub unused: u32,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct FontInstancePlatformOptions {
    pub lcd_filter: FontLCDFilter,
    pub hinting: FontHinting,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub enum FontHinting {
    None,
    Mono,
    Light,
    Normal,
    LCD,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub enum FontLCDFilter {
    None,
    Default,
    Light,
    Legacy,
}

impl Default for FontLCDFilter {
    fn default() -> Self { FontLCDFilter::Default }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct FontInstanceOptions {
    pub render_mode: FontRenderMode,
    pub flags: FontInstanceFlags,
    pub bg_color: ColorU,
    /// When bg_color.a is != 0 and render_mode is FontRenderMode::Subpixel, the text will be
    /// rendered with bg_color.r/g/b as an opaque estimated background color.
    pub synthetic_italics: SyntheticItalics,
}

impl Default for FontInstanceOptions {
    fn default() -> FontInstanceOptions {
        FontInstanceOptions {
            render_mode: FontRenderMode::Subpixel,
            flags: 0,
            bg_color: ColorU::TRANSPARENT,
            synthetic_italics: SyntheticItalics::default(),
        }
    }
}


#[derive(Debug, Copy, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct SyntheticItalics {
    pub angle: i16,
}

impl Default for SyntheticItalics {
    fn default() -> Self {
        Self { angle: 0 }
    }
}

/// Represents the backing store of an arbitrary series of pixels for display by
/// WebRender. This storage can take several forms.
#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub enum ImageData {
    /// A simple series of bytes, provided by the embedding and owned by WebRender.
    /// The format is stored out-of-band, currently in ImageDescriptor.
    Raw(Arc<Vec<u8>>),
    /// An image owned by the embedding, and referenced by WebRender. This may
    /// take the form of a texture or a heap-allocated buffer.
    External(ExternalImageData),
}

impl ImageData {

    pub fn new_raw(data: Vec<u8>) -> Self {
        ImageData::Raw(Arc::new(data))
    }

    pub fn new_external(data: ExternalImageData) -> Self {
        ImageData::External(data)
    }
}
/// Storage format identifier for externally-managed images.
#[repr(u32)]
#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum ExternalImageType {
    /// The image is texture-backed.
    TextureHandle(TextureTarget),
    /// The image is heap-allocated by the embedding.
    Buffer,
}

/// An arbitrary identifier for an external image provided by the
/// application. It must be a unique identifier for each external
/// image.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct ExternalImageId(pub u64);

static LAST_EXTERNAL_IMAGE_ID: AtomicUsize = AtomicUsize::new(0);

impl ExternalImageId {
    /// Creates a new, unique ExternalImageId
    pub fn new() -> Self {
        Self(LAST_EXTERNAL_IMAGE_ID.fetch_add(1, Ordering::SeqCst) as u64)
    }
}

/// Specifies the type of texture target in driver terms.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum TextureTarget {
    /// Standard texture. This maps to GL_TEXTURE_2D in OpenGL.
    Default = 0,
    /// Array texture. This maps to GL_TEXTURE_2D_ARRAY in OpenGL. See
    /// https://www.khronos.org/opengl/wiki/Array_Texture for background
    /// on Array textures.
    Array = 1,
    /// Rectange texture. This maps to GL_TEXTURE_RECTANGLE in OpenGL. This
    /// is similar to a standard texture, with a few subtle differences
    /// (no mipmaps, non-power-of-two dimensions, different coordinate space)
    /// that make it useful for representing the kinds of textures we use
    /// in WebRender. See https://www.khronos.org/opengl/wiki/Rectangle_Texture
    /// for background on Rectangle textures.
    Rect = 2,
    /// External texture. This maps to GL_TEXTURE_EXTERNAL_OES in OpenGL, which
    /// is an extension. This is used for image formats that OpenGL doesn't
    /// understand, particularly YUV. See
    /// https://www.khronos.org/registry/OpenGL/extensions/OES/OES_EGL_image_external.txt
    External = 3,
}

/// Descriptor for external image resources. See `ImageData`.
#[repr(C)]
#[derive(Debug, Clone, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct ExternalImageData {
    /// The identifier of this external image, provided by the embedding.
    pub id: ExternalImageId,
    /// For multi-plane images (i.e. YUV), indicates the plane of the
    /// original image that this struct represents. 0 for single-plane images.
    pub channel_index: u8,
    /// Storage format identifier.
    pub image_type: ExternalImageType,
}

pub type TileSize = u16;

#[derive(Debug, Copy, Clone, PartialEq, PartialOrd)]
pub enum ImageDirtyRect {
    All,
    Partial(LayoutRect)
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum ResourceUpdate {
    AddFont(AddFont),
    DeleteFont(FontKey),
    AddFontInstance(AddFontInstance),
    DeleteFontInstance(FontInstanceKey),
    AddImage(AddImage),
    UpdateImage(UpdateImage),
    DeleteImage(ImageKey),
}

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct AddImage {
    pub key: ImageKey,
    pub descriptor: ImageDescriptor,
    pub data: ImageData,
    pub tiling: Option<TileSize>,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct UpdateImage {
    pub key: ImageKey,
    pub descriptor: ImageDescriptor,
    pub data: ImageData,
    pub dirty_rect: ImageDirtyRect,
}

#[derive(Clone, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct AddFont {
    pub key: FontKey,
    pub font_bytes: Vec<u8>,
    pub font_index: u32,
}

impl fmt::Debug for AddFont {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AddFont {{ key: {:?}, font_bytes: [u8;{}], font_index: {} }}", self.key, self.font_bytes.len(), self.font_index)
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct AddFontInstance {
    pub key: FontInstanceKey,
    pub font_key: FontKey,
    pub glyph_size: Au,
    pub options: Option<FontInstanceOptions>,
    pub platform_options: Option<FontInstancePlatformOptions>,
    pub variations: Vec<FontVariation>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialOrd, PartialEq)]
pub struct FontVariation {
    pub tag: u32,
    pub value: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Epoch(pub u32);

#[derive(Debug, Clone, Copy, Hash, PartialEq, PartialOrd, Eq, Ord)]
pub struct Au(pub i32);

pub const AU_PER_PX: i32 = 60;
pub const MAX_AU: i32 = (1 << 30) - 1;
pub const MIN_AU: i32 = -(1 << 30) - 1;

impl Au {
    pub fn from_px(px: f32) -> Self {
        let target_app_units = (px * AU_PER_PX as f32) as i32;
        Au(target_app_units.min(MAX_AU).max(MIN_AU))
    }
}

pub fn get_font_id(rect_style: &RectStyle) -> &str {
    use crate::ui_solver::DEFAULT_FONT_ID;
    let font_id = rect_style.font_family.as_ref().and_then(|family| family.get_property()?.fonts.get(0));
    font_id.map(|f| f.as_str()).unwrap_or(DEFAULT_FONT_ID)
}

pub fn get_font_size(rect_style: &RectStyle) -> StyleFontSize {
    use crate::ui_solver::DEFAULT_FONT_SIZE;
    rect_style.font_size.and_then(|fs| fs.get_property().cloned()).unwrap_or(DEFAULT_FONT_SIZE)
}


/// Scans the display list for all font IDs + their font size
pub fn scan_styled_nodes_for_font_keys(
    app_resources: &AppResources,
    styled_nodes: &[StyledNode],
    node_data: &[NodeData],
) -> FastHashMap<ImmediateFontId, FastHashSet<Au>> {

    use crate::dom::NodeType::*;

    assert!(styled_nodes.len() == node_data.len());

    let mut font_keys = FastHashMap::default();

    for (styled_node, node_data) in styled_nodes.iter().zip(node_data.iter()) {

        match node_data.get_node_type() {
            Text(_) | Label(_) => {
                let css_font_id = get_font_id(&styled_node.style);
                let font_id = match app_resources.css_ids_to_font_ids.get(css_font_id) {
                    Some(s) => ImmediateFontId::Resolved(*s),
                    None => ImmediateFontId::Unresolved(css_font_id.to_string()),
                };
                let font_size = get_font_size(&styled_node.style);
                font_keys
                    .entry(font_id)
                    .or_insert_with(|| FastHashSet::default())
                    .insert(font_size_to_au(font_size));
            },
            _ => { }
        }
    }

    font_keys
}

/// Scans the display list for all image keys
pub fn scan_styled_nodes_for_image_keys(
    app_resources: &AppResources,
    styled_nodes: &[StyledNode],
    node_data: &[NodeData],
) -> FastHashSet<ImageId> {

    use crate::dom::NodeType::*;

    assert!(styled_nodes.len() == node_data.len());

    let mut images_in_dl = Vec::new();

    for (styled_node, node_data) in styled_nodes.iter().zip(node_data.iter()) {
        // If the node has an image content, it needs to be uploaded
        if let Image(id) = node_data.get_node_type(){
            images_in_dl.push(*id);
        }

        // If the node has a CSS background image, it needs to be uploaded
        let node_has_css_background = || -> Option<ImageId> {
            let background = display_rect.style.background.as_ref().and_then(|bg| bg.get_property())?;
            let css_image_id = background.get_css_image_id()?;
            let image_id = app_resources.get_css_image_id(css_image_id.inner.as_str())?;
            Some(*image_id)
        };

        if let Some(background_id) = node_has_css_background() {
            images_in_dl.push(background_id);
        }

        // If the node has a clip mask, it needs to be uploaded
        if let OptionImageMask::Some(clip_mask) = node_data.get_clip_mask() {
            images_in_dl.push(clip_mask.image);
        }
    }

    images_in_dl.into_iter().collect()
}

// Debug, PartialEq, Eq, PartialOrd, Ord
pub enum AddFontMsg {
    // add font: font key, font bytes + font index
    Font(FontKey, Vec<u8>, u32, LoadedFont),
    Instance(AddFontInstance, Au),
}

impl AddFontMsg {
    pub fn into_resource_update(&self) -> ResourceUpdate {
        use self::AddFontMsg::*;
        match self {
            Font(fk, bytes, index, _) => ResourceUpdate::AddFont(AddFont {
                key: *fk,
                font_bytes: bytes.clone(),
                font_index: *index,
            }),
            Instance(fi, _) => ResourceUpdate::AddFontInstance(fi.clone()),
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub enum DeleteFontMsg {
    Font(FontKey),
    Instance(FontInstanceKey, Au),
}

impl DeleteFontMsg {
    pub fn into_resource_update(&self) -> ResourceUpdate {
        use self::DeleteFontMsg::*;
        match self {
            Font(f) => ResourceUpdate::DeleteFont(*f),
            Instance(fi, _) => ResourceUpdate::DeleteFontInstance(*fi),
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct AddImageMsg(pub AddImage, pub ImageInfo);

impl AddImageMsg {
    pub fn into_resource_update(&self) -> ResourceUpdate {
        ResourceUpdate::AddImage(self.0.clone())
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct DeleteImageMsg(ImageKey, ImageInfo);

impl DeleteImageMsg {
    pub fn into_resource_update(&self) -> ResourceUpdate {
        ResourceUpdate::DeleteImage(self.0.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoadedImageSource {
    pub image_bytes_decoded: ImageData,
    pub image_descriptor: ImageDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoadedFontSource {
    /// Bytes of the font file
    pub font_bytes: Vec<u8>,
    /// Index of the font in the file (if not known, set to 0) -
    /// only relevant if the file is a font collection
    pub font_index: u32,
}

#[repr(C)]
pub struct LoadFontFn { pub cb: fn(&FontSource) -> Option<LoadedFontSource> }
impl_callback!(LoadFontFn);
#[repr(C)]
pub struct LoadImageFn { pub cb: fn(&ImageSource) -> Option<LoadedImageSource> }
impl_callback!(LoadImageFn);

// function to parse the font given the loaded font source
pub type ParseFontFn = fn(&LoadedFontSource) -> Option<(Box<dyn Any>, FontMetrics)>; // = Option<Box<azul_text_layout::Font>>

/// Given the fonts of the current frame, returns `AddFont` and `AddFontInstance`s of
/// which fonts / instances are currently not in the `current_registered_fonts` and
/// need to be added.
///
/// Deleting fonts can only be done after the entire frame has finished drawing,
/// otherwise (if removing fonts would happen after every DOM) we'd constantly
/// add-and-remove fonts after every IFrameCallback, which would cause a lot of
/// I/O waiting.
pub fn build_add_font_resource_updates<T: FontImageApi>(
    app_resources: &AppResources,
    render_api: &mut T,
    pipeline_id: &PipelineId,
    fonts_in_dom: &FastHashMap<ImmediateFontId, FastHashSet<Au>>,
    font_source_load_fn: LoadFontFn,
    parse_font_fn: ParseFontFn,
) -> Vec<(ImmediateFontId, AddFontMsg)> {

    let mut resource_updates = Vec::new();

    for (im_font_id, font_sizes) in fonts_in_dom {

        macro_rules! insert_font_instances {($font_id:expr, $font_key:expr, $font_index:expr, $font_size:expr) => ({

            let font_instance_key_exists = app_resources.currently_registered_fonts[pipeline_id]
                .get(&$font_id)
                .and_then(|loaded_font| loaded_font.font_instances.get(&$font_size))
                .is_some();

            if !font_instance_key_exists {

                let font_instance_key = render_api.new_font_instance_key();

                // For some reason the gamma is way to low on Windows
                #[cfg(target_os = "windows")]
                let platform_options = FontInstancePlatformOptions {
                    gamma: 300,
                    contrast: 100,
                    cleartype_level: 100,
                };

                #[cfg(target_os = "linux")]
                let platform_options = FontInstancePlatformOptions {
                    lcd_filter: FontLCDFilter::Default,
                    hinting: FontHinting::LCD,
                };

                #[cfg(target_os = "macos")]
                let platform_options = FontInstancePlatformOptions::default();

                #[cfg(target_arch = "wasm32")]
                let platform_options = FontInstancePlatformOptions::default();

                let options = FontInstanceOptions {
                    render_mode: FontRenderMode::Subpixel,
                    flags: 0 | FONT_INSTANCE_FLAG_NO_AUTOHINT,
                    .. Default::default()
                };

                resource_updates.push(($font_id, AddFontMsg::Instance(AddFontInstance {
                    key: font_instance_key,
                    font_key: $font_key,
                    glyph_size: $font_size,
                    options: Some(options),
                    platform_options: Some(platform_options),
                    variations: Vec::new(),
                }, $font_size)));
            }
        })}

        match app_resources.currently_registered_fonts[pipeline_id].get(im_font_id) {
            Some(loaded_font) => {
                for font_size in font_sizes.iter() {
                    insert_font_instances!(im_font_id.clone(), loaded_font.font_key, loaded_font.font_index, *font_size);
                }
            },
            None => {
                use self::ImmediateFontId::*;

                // If there is no font key, that means there's also no font instances
                let font_source = match im_font_id {
                    Resolved(font_id) => {
                        match app_resources.font_sources.get(font_id) {
                            Some(s) => s.clone(),
                            None => continue,
                        }
                    },
                    Unresolved(css_font_id) => FontSource::System(css_font_id.clone().into()),
                };

                let loaded_font_source = match (font_source_load_fn.cb)(&font_source) {
                    Some(s) => s,
                    None => continue,
                };

                let (parsed_font, font_metrics) = match (parse_font_fn)(&loaded_font_source) {
                    Some(s) => s,
                    None => continue,
                };

                let LoadedFontSource { font_bytes, font_index } = loaded_font_source;

                if !font_sizes.is_empty() {
                    // loaded_font
                    let font_key = render_api.new_font_key();
                    let loaded_font = LoadedFont {
                        font_key,
                        font: parsed_font,
                        font_metrics,
                        font_instances: FastHashMap::new(),
                    };
                    resource_updates.push((im_font_id.clone(), AddFontMsg::Font(font_key, font_bytes, font_index, loaded_font)));

                    for font_size in font_sizes {
                        insert_font_instances!(im_font_id.clone(), font_key, font_index, *font_size);
                    }
                }
            }
        }
    }

    resource_updates
}

/// Given the images of the current frame, returns `AddImage`s of
/// which image keys are currently not in the `current_registered_fonts` and
/// need to be added. Modifies `last_frame_image_keys` to contain the added image keys
///
/// Deleting images can only be done after the entire frame has finished drawing,
/// otherwise (if removing images would happen after every DOM) we'd constantly
/// add-and-remove images after every IFrameCallback, which would cause a lot of
/// I/O waiting.
#[allow(unused_variables)]
pub fn build_add_image_resource_updates<T: FontImageApi>(
    app_resources: &AppResources,
    render_api: &mut T,
    pipeline_id: &PipelineId,
    images_in_dom: &FastHashSet<ImageId>,
    image_source_load_fn: LoadImageFn,
) -> Vec<(ImageId, AddImageMsg)> {

    images_in_dom.iter()
    .filter(|image_id| !app_resources.currently_registered_images[pipeline_id].contains_key(*image_id))
    .filter_map(|image_id| {
        let image_source = app_resources.image_sources.get(image_id)?;
        let LoadedImageSource { image_bytes_decoded, image_descriptor } = (image_source_load_fn.cb)(image_source)?;
        let key = render_api.new_image_key();
        let add_image = AddImage { key, data: image_bytes_decoded, descriptor: image_descriptor, tiling: None };
        Some((*image_id, AddImageMsg(add_image, ImageInfo { key, descriptor: image_descriptor })))
    }).collect()
}

/// Submits the `AddFont`, `AddFontInstance` and `AddImage` resources to the RenderApi.
/// Extends `currently_registered_images` and `currently_registered_fonts` by the
/// `last_frame_image_keys` and `last_frame_font_keys`, so that we don't lose track of
/// what font and image keys are currently in the API.
pub fn add_resources<T: FontImageApi>(
    app_resources: &mut AppResources,
    render_api: &mut T,
    pipeline_id: &PipelineId,
    add_font_resources: Vec<(ImmediateFontId, AddFontMsg)>,
    add_image_resources: Vec<(ImageId, AddImageMsg)>,
) {
    let mut merged_resource_updates = Vec::new();

    merged_resource_updates.extend(add_font_resources.iter().map(|(_, f)| f.into_resource_update()));
    merged_resource_updates.extend(add_image_resources.iter().map(|(_, i)| i.into_resource_update()));

    if !merged_resource_updates.is_empty() {
        render_api.update_resources(merged_resource_updates);
        // Assure that the AddFont / AddImage updates get processed immediately
        render_api.flush_scene_builder();
    }

    for (image_id, add_image_msg) in add_image_resources.iter() {
        app_resources.currently_registered_images
        .get_mut(pipeline_id).unwrap()
        .insert(*image_id, add_image_msg.1);
    }

    for (font_id, add_font_msg) in add_font_resources {
        use self::AddFontMsg::*;
        match add_font_msg {
            Font(_fk, _bytes, _index, parsed_font) => {
                app_resources.currently_registered_fonts
                .get_mut(pipeline_id).unwrap()
                .insert(font_id, parsed_font);
            },
            Instance(fi, size) => {
                app_resources.currently_registered_fonts
                    .get_mut(pipeline_id).unwrap()
                    .get_mut(&font_id).unwrap()
                    .font_instances.insert(size, fi.key);
            },
        }
    }
}

pub fn build_delete_font_resource_updates(
    app_resources: &AppResources,
    pipeline_id: &PipelineId,
) -> Vec<(ImmediateFontId, DeleteFontMsg)> {

    let mut resource_updates = Vec::new();

    // Delete fonts that were not used in the last frame or have zero font instances
    for (font_id, loaded_font) in app_resources.currently_registered_fonts[pipeline_id].iter() {
        resource_updates.extend(
            loaded_font.font_instances.iter()
            .filter(|(au, _)| !(app_resources.last_frame_font_keys[pipeline_id].get(font_id).map(|f| f.contains(au)).unwrap_or(false)))
            .map(|(au, font_instance_key)| (font_id.clone(), DeleteFontMsg::Instance(*font_instance_key, *au)))
        );
        if !app_resources.last_frame_font_keys[&pipeline_id].contains_key(font_id) || loaded_font.font_instances.is_empty() {
            // Delete the font and all instances if there are no more instances of the font
            resource_updates.push((font_id.clone(), DeleteFontMsg::Font(loaded_font.font_key)));
        }
    }

    resource_updates
}

/// At the end of the frame, all images that are registered, but weren't used in the last frame
pub fn build_delete_image_resource_updates(
    app_resources: &AppResources,
    pipeline_id: &PipelineId,
) -> Vec<(ImageId, DeleteImageMsg)> {
    app_resources.currently_registered_images[&pipeline_id].iter()
    .filter(|(id, _info)| !app_resources.last_frame_image_keys[&pipeline_id].contains(id))
    .map(|(id, info)| (*id, DeleteImageMsg(info.key, *info)))
    .collect()
}

pub fn delete_resources<T: FontImageApi>(
    app_resources: &mut AppResources,
    render_api: &mut T,
    pipeline_id: &PipelineId,
    delete_font_resources: Vec<(ImmediateFontId, DeleteFontMsg)>,
    delete_image_resources: Vec<(ImageId, DeleteImageMsg)>,
) {
    let mut merged_resource_updates = Vec::new();

    merged_resource_updates.extend(delete_font_resources.iter().map(|(_, f)| f.into_resource_update()));
    merged_resource_updates.extend(delete_image_resources.iter().map(|(_, i)| i.into_resource_update()));

    if !merged_resource_updates.is_empty() {
        render_api.update_resources(merged_resource_updates);
    }

    for (removed_id, _removed_info) in delete_image_resources {
        app_resources.currently_registered_images
        .get_mut(pipeline_id).unwrap()
        .remove(&removed_id);
    }

    for (font_id, delete_font_msg) in delete_font_resources {
        use self::DeleteFontMsg::*;
        match delete_font_msg {
            Font(_) => {
                app_resources.currently_registered_fonts
                .get_mut(pipeline_id).unwrap()
                .remove(&font_id);
            },
            Instance(_, size) => {
                app_resources.currently_registered_fonts
                .get_mut(pipeline_id).unwrap()
                .get_mut(&font_id).unwrap()
                .delete_font_instance(&size);
            },
        }
    }
}

pub fn is_image_opaque(format: RawImageFormat, bytes: &[u8]) -> bool {
    match format {
        RawImageFormat::BGRA8 => {
            let mut is_opaque = true;
            for i in 0..(bytes.len() / 4) {
                if bytes[i * 4 + 3] != 255 {
                    is_opaque = false;
                    break;
                }
            }
            is_opaque
        }
        RawImageFormat::R8 => true,
        _ => unreachable!(),
    }
}

// From webrender/wrench
// These are slow. Gecko's gfx/2d/Swizzle.cpp has better versions
pub fn premultiply(data: &mut [u8]) {
    for pixel in data.chunks_mut(4) {
        let a = u32::from(pixel[3]);
        pixel[0] = (((pixel[0] as u32 * a) + 128) / 255) as u8;
        pixel[1] = (((pixel[1] as u32 * a) + 128) / 255) as u8;
        pixel[2] = (((pixel[2] as u32 * a) + 128) / 255) as u8;
    }
}

#[test]
fn test_premultiply() {
    let mut color = [255, 0, 0, 127];
    premultiply(&mut color);
    assert_eq!(color, [127, 0, 0, 127]);
}
