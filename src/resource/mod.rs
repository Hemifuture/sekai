use crate::app::PublishedSphericalPresentation;
use crate::gpu::spherical::SphericalFieldRenderer;
use crate::view::SphericalFieldDisplayState;
mod resource_impl;

#[allow(dead_code)]
pub type SphericalRendererResource = resource_impl::Resource<SphericalFieldRenderer>;
#[allow(dead_code)]
pub type SphericalPresentationResource =
    resource_impl::Resource<Option<PublishedSphericalPresentation>>;
#[allow(dead_code)]
pub type SphericalViewerStateResource = resource_impl::Resource<SphericalFieldDisplayState>;
