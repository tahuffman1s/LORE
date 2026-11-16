// The C interface between `src/ffi.rs` and the FidelityFX SDK.
//
// Every struct here has a `#[repr(C)]` twin in `src/ffi.rs`; change both together.

#ifndef LORE_FSR_SHIM_H
#define LORE_FSR_SHIM_H

#include <stdint.h>
#include <vulkan/vulkan.h>

#ifdef __cplusplus
extern "C" {
#endif

// Returned by lore_fsr_create when an upscaler already exists on another device.
#define LORE_FSR_ERROR_SECOND_DEVICE ((int32_t)0x7F550001)

// Vulkan functions looked up by the game, which the SDK would otherwise need
// the Vulkan library for at link time.
typedef struct LoreFsrLoader
{
    PFN_vkGetDeviceProcAddr                  getDeviceProcAddr;
    PFN_vkEnumerateDeviceExtensionProperties enumerateDeviceExtensionProperties;
    PFN_vkGetPhysicalDeviceMemoryProperties  getPhysicalDeviceMemoryProperties;
    PFN_vkGetPhysicalDeviceProperties        getPhysicalDeviceProperties;
    PFN_vkGetPhysicalDeviceProperties2       getPhysicalDeviceProperties2;
    PFN_vkGetPhysicalDeviceFeatures          getPhysicalDeviceFeatures;
    PFN_vkGetPhysicalDeviceFeatures2         getPhysicalDeviceFeatures2;
} LoreFsrLoader;

typedef struct LoreFsrCreate
{
    VkDevice         device;
    VkPhysicalDevice physicalDevice;
    uint32_t         maxRenderWidth;
    uint32_t         maxRenderHeight;
    uint32_t         maxOutputWidth;
    uint32_t         maxOutputHeight;
} LoreFsrCreate;

// One image handed to the upscaler. `image` is a VkImage; 0 means "none".
typedef struct LoreFsrImage
{
    uint64_t image;
    uint32_t format; // VkFormat
    uint32_t usage;  // VkImageUsageFlags the image was created with
    uint32_t width;
    uint32_t height;
} LoreFsrImage;

typedef struct LoreFsrDispatch
{
    VkCommandBuffer commands;
    LoreFsrImage  color;
    LoreFsrImage  depth;
    LoreFsrImage  motion;
    LoreFsrImage  output;
    float           jitterX;
    float           jitterY;
    float           motionScaleX;
    float           motionScaleY;
    float           sharpness; // 0 switches sharpening off
    float           frameMilliseconds;
    float           cameraNear;
    float           cameraFovY; // radians
    uint32_t        reset;
} LoreFsrDispatch;

typedef struct LoreFsr LoreFsr;

int32_t lore_fsr_create(const LoreFsrLoader* loader, const LoreFsrCreate* create, LoreFsr** out);
void    lore_fsr_destroy(LoreFsr* fsr);
int32_t lore_fsr_dispatch(LoreFsr* fsr, const LoreFsrDispatch* frame);
int32_t lore_fsr_jitter_offset(float* x, float* y, int32_t index, int32_t phaseCount);

#ifdef __cplusplus
}
#endif

#endif // LORE_FSR_SHIM_H
