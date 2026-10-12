# Working sRGB profile

`srgb.icc` is the profile generated locally with Pillow ImageCms/LittleCMS `createProfile("sRGB")`, previously used as a project-resource input fixture. It is now an application asset for PNG/JPEG/TIFF output. The profile copyright field reads "No copyright, use freely". It has the ICC D50 PCS illuminant and aligned tag offsets; no third-party profile download was used.

Do not delete this file as a generated test result. Its bytes are embedded in the executable and identify the native working-profile import path.

`srgb-gray.icc` is the neutral working-gray profile derived from that same local profile: GRAY header, D50 PCS, original sRGB tone curve and white point/copyright/description tags. Its tag table is rebuilt with four-byte alignment and unspecified profile ID. Independent LittleCMS maps all 256 gray input codes to exactly the same neutral sRGB codes.
