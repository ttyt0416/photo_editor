"""QA-only C OpenJPEG decoder from the installed PyAV dependency (not shipped).
Bind the public OpenJPEG C ABI, read components before any 8-bit conversion.
"""
import ctypes as C, sysconfig, os
from pathlib import Path
import numpy as np
u=C.c_uint32; i=C.c_int32; ptr=C.c_void_p
class Parameters(C.Structure):
    _fields_=[('cp_reduce',u),('cp_layer',u),('infile',C.c_char*4096),('outfile',C.c_char*4096),('decod_format',i),('cod_format',i)]+[(n,u) for n in ['DA_x0','DA_x1','DA_y0','DA_y1']]+[(n,i) for n in ['m_verbose','tile_index','nb_tile_to_decode','jpwl_correct','jpwl_exp_comps','jpwl_max_tiles','flags']]
class Component(C.Structure):
    _fields_=[(n,u) for n in ['dx','dy','w','h','x0','y0','prec','bpp','sgnd','resno_decoded','factor']]+[('data',C.POINTER(i)),('alpha',C.c_uint16)]
class Image(C.Structure):
    _fields_=[(n,u) for n in ['x0','y0','x1','y1','numcomps']]+[('color_space',i),('comps',C.POINTER(Component)),('icc_profile_buf',ptr),('icc_profile_len',u)]
folder=Path(sysconfig.get_paths()['purelib'])/'av.libs'
_library=list(folder.glob('*openjp2*.dll')); assert len(_library)==1
_directory=os.add_dll_directory(str(folder))
lib=C.CDLL(str(_library[0]))
def bind(name,args,ret):
    function=getattr(lib,name);function.argtypes=args;function.restype=ret;return function
version=bind('opj_version',[],C.c_char_p)().decode()
default=bind('opj_set_default_decoder_parameters',[C.POINTER(Parameters)],None)
create=bind('opj_create_decompress',[i],ptr)
setup=bind('opj_setup_decoder',[ptr,C.POINTER(Parameters)],i)
stream_create=bind('opj_stream_create_default_file_stream',[C.c_char_p,i],ptr)
read_header=bind('opj_read_header',[ptr,ptr,C.POINTER(C.POINTER(Image))],i)
decode=bind('opj_decode',[ptr,ptr,C.POINTER(Image)],i)
end=bind('opj_end_decompress',[ptr,ptr],i)
image_destroy=bind('opj_image_destroy',[C.POINTER(Image)],None)
stream_destroy=bind('opj_stream_destroy',[ptr],None)
codec_destroy=bind('opj_destroy_codec',[ptr],None)
def component_planes(path, layers=0):
    parameters=Parameters(); default(C.byref(parameters)); parameters.cp_layer=layers
    codec=create(2 if path.suffix=='.jp2' else 0); assert codec
    stream=None; image=C.POINTER(Image)()
    try:
        assert setup(codec,C.byref(parameters))
        stream=stream_create(str(path).encode('utf-8'),1); assert stream
        assert read_header(stream,codec,C.byref(image))
        assert decode(codec,stream,image) and end(codec,stream)
        data=[]
        for n in range(image.contents.numcomps):
            comp=image.contents.comps[n]
            data.append(np.ctypeslib.as_array(comp.data,shape=(comp.w*comp.h,)).copy().reshape(comp.h,comp.w)
                        if comp.w and comp.h else np.empty((comp.h,comp.w),dtype='i4'))
        return data
    finally:
        if image: image_destroy(image)
        if stream: stream_destroy(stream)
        codec_destroy(codec)

def components(path, layers=0):
    return np.stack(component_planes(path,layers),axis=2)
