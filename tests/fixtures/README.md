# 테스트 이미지 출처

- `flat-red.heic`: `heif-oxide 0.1.0`의 `testdata/flat_red_64.heic`를 복사한 64×64 RGB 예제. 원본 MIT 고지는 `HEIF-LICENSE-MIT`에 보존했다. upstream: https://github.com/dan335/heif-oxide
- `independent.jp2`: Pillow/OpenJPEG로 별도로 만든 7×5 RGBA 무손실 합성 이미지. `(x,y)`의 색은 `[x*30, y*40, 120]`, 알파는 x<3이면100, 그 외255다. 앱의 JP2 디코더와 독립적인 인코더에서 만든 테스트 데이터다.
- `srgb.icc`: Pillow ImageCms/lcms의 `createProfile("sRGB")`로 생성한 테스트 프로파일. PSD 이미지 리소스의 원본 바이트 보존을 검증하며 현재 색상 변환 지원을 뜻하지 않는다.
- `independent-layered16.psd`, `independent-layered32.psb`, `independent-flat32.psd`: 앱 코드를 사용하지 않고 Python 표준 라이브러리 `struct/zlib`와 [Adobe 형식 명세](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)로 작성한 합성 예제. 생성기는 `generate_psd_fixtures.py`다. Lr16/Lr32 전역 레이어, 16/32비트 ZIP 예측, 음수 위치, HDR/음수 RGB·알파와 합성 전용 ZIP 예측을 검증한다. 실제 Photoshop에서 만든 예제와 구분한다.

개인 사진은 포함하지 않았다. 추가 JPEG XL, 실제 Photoshop 생성 PSD/PSB, 실제 카메라 RAW/압축, CMYK와 HDR/ICC 복합 예제 등은 추가 검증 대상이다.

`verify_adjustment_outputs.py`는 `cargo test --offline`이 생성한 `qa-output/adjustment-layer/selected-hdr.rphoto/.psd/.psb`를 Python 구조 검사와 Pillow 마스크 읽기로 확인한다. 노출·분수 선택/페더·마스크·불투명도의 독립 수식을 사용하며 Rust 렌더러/로더를 호출하지 않는다. HDR·음수 RGB·알파와 출력의 픽셀 한 레이어 변환을 검사한다. Photoshop 네이티브 조정 레이어 왕복 검증과 구분한다.

`qa-output/filter-bounds/expanded8/16/32.rphoto/.psd/.psb`는 Rust의 3×3 원본에서 가우시안·모션 블러를 경계 밖으로 확산한 생성 예제다. 외곽/알파·선곱 RGB·정밀도·원본 보존 왕복 검사를 수행한다. Photoshop 생성 예제나 외부 앱 왕복 결과는 아니다.

`qa-output/tonal-contrast/selected-hdr.rphoto/.psd/.psb`는 12×6 HDR/음수 RGB·분수 알파 원본에 3개 톤과 선택/페더를 적용한 생성 예제다. `verify_tonal_outputs.py`가 Python Gaussian/톤 가중치/분수 범위 수식으로 PSD/PSB 합성 픽셀을 독립 검증한다. Photoshop 생성/왕복 예제는 아니다.

`qa-output/svg-paths/edited.rphoto/.psd/.psb`는 그룹/뷰박스·CSS 그라디언트·참조·다중 하위 경로/원호와 레이어 원근을 가진 SVG의 경로 분할/삭제 생성 예제다. 프로젝트는 XML 편집 구조를, PSD/PSB는 변환한 픽셀 외관을 검사한다. 원본 SVG/Photoshop 외부 왕복 예제는 아니다.

`qa-output/svg-native/native.svg/.rphoto`는 두 SVG의 중복 ID·공통 클래스/CSS·:root·rect/circle/use/그룹/그라디언트/메타데이터/주석을 원래 구조로 출력한 생성 예제다. `verify_svg_native_outputs.py`가 XML 요소·고유 ID·URL/href 관계·CSS 범위와 프로젝트 원래 자산을 독립 확인한다. 외부 브라우저/편집기 실제 왕복 예제는 아니다.

- `qa-output/svg-viewport/expanded.rphoto/.svg/.psd/.psb`: SVG 경계 밖 경로 확장, 원래 루트 스타일/도형/주석과 기존 좌표 보존, 출력 다시 열기 예제. Rust 검사로 반복 확장/연결·독립 마스크/보정 범위/16비트 작업 버퍼/이력/오류 원본 보존을 확인한다. SVG 렌더링은 현재 8비트이며 외부 편집기 검증을 뜻하지 않는다.

- `qa-output/svg-objects`: 캔버스 도형/그룹/use 이동·크기·회전과 마스크/정밀도/뷰포트/저장 예제. `native8.rphoto`/`native.svg`는 원래 SVG 요소·기하·참조 구조를 보존하고 `verify_svg_object_outputs.py`가 독립 XML 검사를 수행한다. `native.rphoto/.psd/.psb`와 `precision-flattened.svg`는 32비트/독립 마스크에 따른 변환 사례. 로컬 usvg 패치의 참조/중첩 SVG 좌표를 규격상 기대 값과 렌더 픽셀로 검사한다. 외부 앱 검증은 별도.

- `qa-output/webp-output`: WebP 손실 품질 5종 × 압축 노력 3종과 무손실 출력, RGBA 원본 PNG. `verify_webp_outputs.py`가 Pillow로 독립 디코딩하고 실제 RIFF/VP8/VP8L/ALPH·바이트 크기·픽셀/알파/고품질 오차를 확인한다.

- `qa-output/tiff-output`: TIFF 호환52종의 압축/8·16·32비트/정수 수평 예측/classic·BigTIFF/300ppi/HDR·음수·비연관 알파 예제. `verify_tiff_outputs.py`가 tifffile로 IFD를 읽고 별도 Python LZW/PackBits·zlib 및 예측 역변환으로 픽셀을 독립 복원한다. `source32.le`는 little-endian f32 합성 원본으로 알려진 수식과 교차 확인한다. 다중 페이지/ICC·외부 앱은 별도 목표. 이전 무압축/PackBits+수평 예측8종은 독립 디코더 호환성 오류를 확인해 현재 저장에서 거부한다.

- `qa-output/avif-output`: AVIF 품질/8·10비트/YCbCr BT.601/709·RGB 18종과 속도·알파 품질 3종. Rust에서 AV1 시퀀스 정밀도/α와 고정밀 값을 확인하고 `verify_avif_outputs.py`가 독립 Pillow 디코딩 및 BMFF pixi/av1C/명시 colr를 검사한다. BT.601 기본값은 AV1 색 정보로 표현되어 colr가 생략될 수 있다. 품질 100은 무손실 보장이 아니며 12비트/HDR·ICC/메타데이터/다중 프레임은 별도 목표.

## JPEG 출력/독립 입력 (순차 39)

`verify_jpeg_outputs.py`는 Rust 출력 32조합의 JPEG 마커/샘플링/재시작/ppi와 스캔 2/8/64를 Pillow/libjpeg로 독립 복원하며 최적화 총 용량을 확인한다. `jpeg-input`은 Pillow 12.1.0이 자체 생성한 RGB 수식 패턴(12×8, 품질100, 4:4:4), EXIF 방향1–8/식별용 ICC 바이트, 회색조83과 CMYK(0,80,140,30)이다. 대응 PNG는 Pillow 복원 및 EXIF 적용 기대값이며 외부 사진/개인정보가 없다. 전체 실제 JPEG 변형/ICC 색 변환 검증을 뜻하지 않는다.

## GIF 옵션 (순차 40)

`verify_gif_outputs.py`는 자체 수식 색상/알파/램프 예제를 GIF 블록 검사와 Pillow 독립 복원으로 확인한다. 팔레트2/16/256·디더링/투명/인터레이스24조합, 투명 검정 충돌/정확한 소수 색상, 경계1/127/128/255, 완전 투명과 행 재배치, 램프의 국소 평균 개선을 확인한다. 외부 이미지/개인정보가 없고 애니메이션/모든 실제 GIF 변형 검증을 뜻하지 않는다.

## 실제 출력 미리보기 (순차41)

`verify_export_previews.py`는 수식 RGBA 예제의16형식 실제 파일 크기/표시 크기를 `manifest.json`과 대조하고12개의 Pillow 코덱으로 다시 읽어 표시 PNG와 픽셀을 비교한다. JPEG 디코더의 IDCT 반올림 차이는3바이트 이내를 검사한다. EXR/HDR/Farbfeld/SVG는 독립 픽셀 검사에서 제외하고 Rust 왕복 검사로 구분한다. 표시 알파/축소/부분 좌표·원본 불변·오류 정리와 백그라운드/입력 경쟁은 Rust 검사에 있다. 외부 파일/개인정보가 없다.

순차43의 `qa-output/font-picker/fonts.rphoto`는 코드로 만든 문자별 글꼴/기울임과 누락 이름 보존 예제다. 설치된 Windows 글꼴은 읽어서 검사하며 글꼴 파일을 예제/프로젝트에 포함하지 않는다.

JPEG XL 예제6종은 [libjxl/conformance](https://github.com/libjxl/conformance) 커밋 b1d0f990b03e57bf6d137c365cd5dc8b470b9191에서 가져왔다. `jxl/manifest.json`에 입력/기준 ICC/NPY의 원래 URL·해시와 압축 전후 해시·치수를 기록한다. NPY float는 무손실 zlib로만 압축했다. BSD-3-Clause 고지를 `jxl/LICENSE`에 유지하고 alpha_triangles/sunset_logo의 Jon Sneyers CC0 source 고지도 함께 보관한다. `verify_jxl_inputs.py`는 Rust 출력의 전체 픽셀을 원래 float 기준과 별도로 비교한다. alpha_premultiplied의 RGB는 jxl-oxide upstream 채널별 peak/RMS 기준이며 더 엄격한 코퍼스 공통 기준 통과는 주장하지 않는다. 실제 HDR/ICC 디스플레이·전체 JXL 변형/ISO 적합성 검증은 남음.

- `qa-output/tiff-predictor`: classic/BigTIFF×LZW/Deflate4수준×3치수의32비트 Predictor3 TIFF24종과 비교용 일반 압축. `verify_tiff_predictor.py`는 별도 LZW/zlib·NumPy 평면 역변환으로 HDR/음수/signed zero/알파 전체 float 비트를 원본 `.le`와 비교하고 여러 스트립과 압축 크기를 확인한다.

- `gray-psd`: 별도 struct/zlib 인코더로 만든 회색조 PSD/PSB72종. [예제 안내](gray-psd/README.md)에 원본 수치/압축/알파/그룹·합성 ICC 헤더의 목적과 Adobe 출처를 기록했다. psd-tools1.10.9와 `verify_gray_psd_inputs.py`로 원본/96개 Rust 배열 전체 비트 및 중성 RGB 출력3종을 독립 비교한다. 실제 Photoshop 예제/왕복을 뜻하지 않는다.

- `rgb-psd-order`: psd-tools1.10.9의 별도 쓰기로 만든 외부48종/이전 개발 순서48종. [예제 안내](rgb-psd-order/README.md)에 순서/샘플/폴더 별도 블렌드와 출처/해시를 기록했다. `verify_psd_order_outputs.py`가 새48출력·중첩2종·기존24그룹 출력을 독립 복원한다. 순차48에서는 저장된 합성 평면을 확인하며 white matte 표시 표현은 다음49번 대상이다.

- `psd-matte`: 별도 struct/zlib의 흰 배경 RGB 평면48종/이전 straight24종. [예제 안내](psd-matte/README.md)에 tiny α/HDR/원본/표시 기준과 외부 float 리더 어댑터를 기록했다. `verify_psd_matte_outputs.py`와 확대된 PSD/보정/톤 독립 검사가 흰 배경 표현과 복원 표시를 검증한다. 실제 Photoshop 왕복과 구분한다.

순차50의 `verify_gray_psd_outputs.py`는 기존 독립 회색조72개/컬러48개 입력에서 나온 네이티브 회색조72개와 두 변환 방식96개·보정 출력을 검사한다. 원본/알파 비트·GRAY ICC/구조·수치 휘도 수식과 저장 평면을 psd-tools/NumPy로 비교한다. 합성 테스트 ICC는 실제 색상 변환 프로파일이 아니며 Photoshop 실제 왕복과 구분한다.

순차51의 합성 DNG20종은 `raw/README.md`, 자체 encoder/manifest와 `verify_raw_outputs.py`에 기록했다. 센서 바이트를 독립 파싱해 40개 촬영/중립+사용자 EV·행렬/곡선/8방향/정밀도·자르기와 프로젝트의 RAW 원본을 비교한다. 실제 카메라 전수와 구분한다.

순차52 색상 믹서의 자체 생성 입력/계산3650개와 RAW 선택 영역 출력은 `qa-output/color-mixer`에 보관한다. `verify_color_mixer_outputs.py`는 colorsys/NumPy와 psd-tools로 별도 계산/32비트 평면을 비교한다. 실제 Adobe 외관 비교와 구분한다.

순차53 자체 색상 그레이딩 입력/계산6270개, 선택 영역 RAW 및 PSD/PSB 출력은 `qa-output/color-grading`에 보관하며 `verify_color_grading_outputs.py`가 colorsys/NumPy와 psd-tools로 독립 비교한다. 실제 Adobe 외관/ICC 검증과 구분한다.

순차54 노이즈 감소 자체 입력117픽셀×10설정과 RAW 선택 영역/PSD·PSB 예제는 `qa-output/denoise`이며 `verify_denoise_outputs.py`가 NumPy 별도 가로/세로 경계 보존 필터로 비교한다. 합성 잡음·계단 경계와 실제 카메라 화질/성능을 구분한다.

순차55 세부 선명화의 자체 입력117픽셀×10설정과 RAW 선택 영역/PSD·PSB 예제는 `qa-output/detail-sharpen`이며 `verify_detail_sharpen_outputs.py`가 NumPy 별도 f64 가우시안/알파/경계 제한 수식으로 비교한다. 실제 카메라/외부 앱 화질 검증과 구분한다.

순차56 RAW 미리보기의6크기 합성 float 원본·6전체/24부분 PNG는 `qa-output/raw-preview`이며 `verify_raw_previews.py`가 NumPy/Pillow로 알파 면적 평균·SDR 변환/정규화 위치·정확한 부분 픽셀을 비교한다. 원본 RAW20종 독립 검사는 별도로 유지한다.

순차57 `generate_raw_metadata_inputs.py`가 기존 합성 DNG 센서 바이트를 유지해 별도 EXIF IFD/두 byte order/ISO32/잘못된 분수·긴 이름4종을 `raw-metadata`에 생성한다. `verify_raw_metadata.py`는 자체 TIFF/EXIF 파서로 원래 분수/렌즈/ISO·센서 바이트 및 `qa-output/raw-metadata` 프로젝트의 RAW 자산/메타데이터를 비교한다. 원본20종은 수정하지 않는다.

순차58 `lens-profiles/synthetic.xml`은 자체 모델/카메라/오류 자료이며 `slr-nikon.xml`은 고정 커밋의 공식 Lensfun CC BY-SA3.0 검사 데이터(변경 없음/원래 주석/라이선스 포함)다. 출처/해시는 폴더 README에 있다. `verify_lens_profile_outputs.py`는 NumPy 별도4모델/3보간·5265픽셀·RAW 자산/범위/PSD·PSB를 비교한다. 전체 네이티브 Lensfun/실제 렌즈 화질을 주장하지 않는다.

순차59 자체 `lens-profiles/tca.xml`은3색수차 모델/전용·중복/잘못된 값 자료다. `verify_lens_tca_outputs.py`는 별도 NumPy 수식/float 보간(공유 `lens_profile_math.py`)으로72경우/8424픽셀과 녹색/알파·왜곡 적용 순서·RAW 범위/원본/PSD·PSB를 비교한다.

순차60 자체 `lens-profiles/vignetting.xml`은 PA/ACM/조리개·거리·중복/잘못된 값 자료다. `verify_lens_vignetting_outputs.py`는 별도 NumPy 선형 sRGB/정규화/보정 순서의90경우/10530픽셀과 RAW 범위/자산/PSD·PSB를 비교한다.

순차61 자체 `lens-profiles/interpolation.xml`은4왜곡/3색수차/4초점거리/두 교정 그룹 자료다. `verify_lens_interpolation_outputs.py`는 별도 XML 파싱/4×4cubic 경계 연립방정식으로40계산·계수/실제 초점거리·RAW 범위/원본/PSD·PSB를 비교한다.

순차62 자체 `lens-profiles/vignette-interpolation.xml`은 PA/ACM·3축/단일 초점거리/교정 그룹 자료다. `verify_lens_vignette_interpolation_outputs.py`는 별도 XML/NumPy Shepard 거리 가중으로21조건의 계수·RAW 보정 픽셀/자산/PSD·PSB를 비교한다.

순차66 기본 점 보정·범위 혼합의 HDR/음수/MAX/작은 알파 검사는 `qa-output/hdr-adjustments`에 기록한다. `verify_hdr_adjustment_outputs.py`가600기본 보정 픽셀과18000알파 가중 혼합의 독립 NumPy 수식·프로젝트 설정을 확인한다. 전체 합성/공간 필터/색 관리 완료를 뜻하지 않는다.

순차67의 공통 보간 자료는 `qa-output/float-sampling`이며 `verify_float_sampling_outputs.py`가36입력/보간 경우의324결과와32비트 PSD·PSB 작은 알파/HDR 값을 독립 확인한다. 광학용 공유 Python 누산도 f32 표현 범위 제한을 명시한다.

순차68 자료는 `qa-output/float-composite`이며 `verify_float_composite_outputs.py`가6480Normal 조합/1024정수 해시 디졸브 위치·HDR 레이어15/통과 그룹15픽셀과 저장 그룹을 독립 확인한다. 다른 블렌드/공간 필터/색 관리 완료를 뜻하지 않는다.

순차69 자체 생성 자료는 `qa-output/float-blur`이며 `verify_float_blur_outputs.py`가 SciPy 분리 합성곱으로200조건/2600픽셀을 독립 비교한다. 극값 상쇄의 입력 크기 비례 허용오차는 순차 검증 기록에 공개했다. 외부 사진/Photoshop 왕복이나 대형 성능 완료를 뜻하지 않는다.

순차70 자체 생성 자료는 `qa-output/spatial-hdr`이며 `verify_spatial_hdr_outputs.py`가 방향/선 샘플과 SciPy Gaussian/선명화400조건·5200픽셀을 독립 계산한다. 저장/이력/부분 합성 자료는 외부 Photoshop 왕복과 구분하며 수치 허용오차는 검증 기록을 따른다.

순차71 자체 자료는 `qa-output/bloom-hdr`이며 `verify_bloom_hdr_outputs.py`가320조건/4160픽셀의 밝기 추출/알파 유지/경계 확산을 독립 계산한다. `float_filter_math.py`의 SciPy Gaussian 참조는 순차70과 공유한다. 실제 사진 화질/Photoshop 왕복·색 관리 완료와 구분한다.

순차72 자체 자료는 `qa-output/tonal-hdr`이며 `verify_tonal_hdr_outputs.py`가200조건/2600픽셀의 밝기/톤 가중치/양수·음수 강도를 독립 계산한다. 알파 보존·저장/복원 자료는 실제 Photoshop 왕복과 구분한다.

순차73 자체 자료는 `qa-output/blend-hdr`이며 `verify_blend_hdr_outputs.py`가1600색상/48000합성/375레이어 픽셀을110자리 Decimal로 독립 계산한다. W3C 수식과 앱의 추가 모드/기존 HDR 제한을 구분하고 상쇄 조건별 허용오차는 검증 기록에 공개한다. 실제 Photoshop 왕복/ICC 완료를 뜻하지 않는다.

순차74 자료는 `qa-output/raw-white-balance`. `verify_raw_white_balance_outputs.py`가 자체20DNG의274센서 패치/30000출력 픽셀을 독립 파싱·NumPy 행렬로 계산한다. 4채널은 별도 합성 수식 검사이며 실제4색 카메라 디코딩이나 Photoshop 왕복 완료를 뜻하지 않는다.

순차75 자체/기존 입력 자료는 `qa-output/svg-assets`. `verify_svg_asset_outputs.py`가16입력의 XML/base64/정확한 원본/프로젝트 자산·Pillow 및 별도 생성 수식을 비교한다. QOI/PAM은 알려진 생성 수식이며 추가 HEIC/JP2/JXL/DNG/PSD의 독립 코덱 검증은 앞선 각 자료를 따른다. 외부 에디터 왕복/전체 SVG 적합성 완료와 구분한다.

순차76 자체 Pillow PNG EXIF/APNG/WebP 자료는 `svg-images`, 입력/출력은 `qa-output/svg-assets/normalized`, `compressed`. `verify_svg_normalized_outputs.py`가20입력의 별도 방향/첫 화면 픽셀·원본/프로젝트·gzip/XML을 확인한다. 기존 JPEG 골드의 출처는 해당 안내를 따르며 전체 SVG 적합성/애니메이션 재생/색 관리 완료를 뜻하지 않는다. 출력 미리보기 검사는 SVGZ를 포함해17형식으로 확장했다.

순차77 자체 자료는 `qa-output/svg-assets/replace`, `panel`, `app`. `verify_svg_replacement_outputs.py`가9속성·다른 요소/defs/CSS/주석·정확한 원본·프로젝트 소스·압축 SVG를 독립 비교한다.880×620/1·2배 DPI headless 파일 경로/교체/최종 적용·취소는 실제 Windows 파일 선택 창 검증과 구분한다.

순차78 자체/기존 원본 자료는 `qa-output/svg-assets/extract`, `extract-nested`. `verify_svg_extraction_outputs.py`가12입력의 정확한 원본 SHA·EXIF/기본 화면/애니메이션 전체 프레임·프로젝트/gzip/중첩 내장 SVG·취소 후 이전 출력을 확인한다. 외부 원본의 출처는 각 기존 폴더 안내를 따르고 실제 파일 선택 창/애니메이션 재생·전체 색 관리 완료와 구분한다.

순차79: 자체20DNG/15광원·120현상 결과는 `verify_raw_temperature_outputs.py`가 CIE 원본 표/정확한 SI 상수/NumPy 스펙트럼·센서 행렬과 정밀도/방향/프로젝트를 독립 비교한다. 출처·변경 없는 CSV/메타데이터·CC BY-SA 4.0은 `data/cie/README.md`에 있다. 실제 카메라/다중 광원/ICC 전수 완료를 뜻하지 않는다.

순차80: 자체 Lensfun XML을 QA 사본에서 변경/삭제/손상/복구하고 `verify_lens_watch_outputs.py`로13프로파일 원본 XML/계수·설정 보존을 독립 파싱한다. 원래 fixture는 수정하지 않으며 외부 DB/실제 카메라 전수 완료를 뜻하지 않는다.

순차81: `generate_raw_hdr_inputs.py`가 자체3선형/흑백·자르기/방향 DNG에 최대 유한/작은/0 센서와 카메라 행렬을 기록한다. `verify_raw_hdr_outputs.py`가36WB·EV 결과3168픽셀을 독립64비트 센서/행렬/곡선/저장 범위로 비교한다. 실제 센서/4색 CFA/ICC 전수 예제가 아니다.

순차82: 자체45 DNG/9광원·3/4채널 선형과 RGB/Cyan·CMYG·순서 변경4색 CFA는 `generate_raw_color_inputs.py`로 인코딩한다. `verify_raw_color_outputs.py`가180결과14688픽셀의 센서/Bradford/NumPy 의사 역행렬·원본/프로젝트를 독립 비교한다. CFA 내부 비교이며 실제 센서/경계·전체 DNG/ICC 전수 완료를 뜻하지 않는다.

순차83: 자체18 이중 광원 DNG는 `generate_raw_dual_inputs.py`로 인코딩한다. `verify_raw_dual_outputs.py`가108결과8352픽셀의 역CCT 보간·동적 Bradford/NumPy 역행렬·SciPy CCT/센서/프로젝트를 독립 비교한다. 실제 센서/전체 DNG/ICC 전수 완료를 뜻하지 않는다.

순차84: 같은 자체18 DNG의 촬영 WB/54EV 결과4176픽셀은 `verify_raw_camera_profile_outputs.py`가 SVD 카메라 중립 역변환과 brentq CCT 근 찾기로 독립 비교한다. 실제 센서/전체 DNG/ICC 전수 완료를 뜻하지 않는다.

순차85: 자체18이중 광원의54사용자 배율/100샘플11568픽셀은 `verify_raw_custom_profile_outputs.py`가 센서 평균과 SVD/Brent CCT로 독립 비교한다. 단일4채널27DNG/9광원·4색 목록의 모든 배율/프로젝트도 비교해 `verify_raw_color_outputs.py`는207결과17064픽셀을 검사한다. 실제 센서/전체DNG·ICC 전수 완료를 뜻하지 않는다.

순차86: `generate_raw_analog_inputs.py`로 자체18컬러 DNG의 사전 하드웨어 배율을 센서/중립 태그에 반영하고 단위 대조 파일을 만든다. `verify_raw_analog_outputs.py`가90결과7920픽셀·하드웨어 센서/태그·SVD/스펙트럼·원본/프로젝트를 독립 비교한다. 실제 센서/전체DNG/ICC 전수 완료를 뜻하지 않는다.

순차87: `generate_raw_white_inputs.py`가 자체54DNG의 촬영 중립값 태그를 A/D65 좌표로 교체하고 사전 배율 비단위/단위/누락을 포함한다. `verify_raw_white_outputs.py`가270결과23760픽셀·좌표 배율·SVD/스펙트럼/CCT·촬영 정보·정확한 센서 바이트/원본/프로젝트를 독립 비교한다. 실제 센서/전체DNG/ICC 전수 완료를 뜻하지 않는다.

순차88: 자체40 DNG를 `generate_raw_calibration_inputs.py`로 생성한다. `verify_raw_calibration_outputs.py`가200결과19200픽셀/UTF-8 서명·독립 CC/CM 보간·AB 순서/촬영 xy·SVD/CCT 근·원본과 v9/v11 촬영 정보를 비교한다. 기존 자체 센서 스트립은 변경하지 않는다. 실제 카메라/Forward/Reduction/대체·3광원/룩·ICC는 별도 남은 목표다.

순차89: `qa-output/file-watch-content`의 자체131073바이트/한글 경로·같은 메타데이터 중간/마지막 변경은 `verify_file_watch_content_outputs.py`의 hashlib로 지문을 독립 비교한다. 원본 복구/프로젝트·PSD·PSB 저장 충돌·headless 편집/이력과 실제 백그라운드 취소는 Rust 검사로 확인한다. 전체 대형/동시 외부 쓰기 전수 검증은 별도다.

순차90: `qa-output/jpeg2000-output`의 자체12원본/216JP2·J2K/904500픽셀을 `verify_jpeg2000_outputs.py`에서 C OpenJPEG/독립 박스로 검증한다. 전체 색·알파/8·16비트 무손실 및 Rust/C 손실 디코더 결과 차이는0이다. `verify_export_previews.py`는19파일/14Pillow코덱·SVG/XML/gzip을 비교한다. 전체 JPEG2000/ICC/추가 색 모드/대형 성능 인증은 별도다.

순차91: `qa-output/jpeg2000-output/progressive`의 자체60파일/140품질 읽기430080픽셀은 `verify_jpeg2000_progressive_outputs.py`의 C OpenJPEG로 비교한다. `generate_jpeg2000_progressive_inputs.py`가 만든 자체20C입력의60부분/전체 기대값도 Rust에서 읽는다. 5패킷 순서/1·4·100단계/무손실·손실/JP2·J2K·RGBA와 최종 무손실 픽셀·COD를 확인하며 전체 ISO/대형/외부 뷰어 검증은 구분한다.


순차92–97: JPEG2000 자체 회색조96파일·타일128파일·표시 해상도40파일·부호/정밀도84입력·sYCC6입력·축소 샘플링48입력의 검증은 `verify_jpeg2000_gray_outputs.py`, `verify_jpeg2000_tiled_outputs.py`, `verify_jpeg2000_resolution_outputs.py`, `verify_jpeg2000_input_outputs.py`, `verify_jpeg2000_sycc_outputs.py`, `verify_jpeg2000_sampling_outputs.py`에 있다. 설치된 PyAV의 C OpenJPEG2.5.2 공개 ABI로 원시 성분을 읽는 `jpeg2000_c_reference.py`는 QA 전용이며 배포하지 않는다. 밝기/색/정밀도와 PNG는 별도 NumPy 정수/벡터 계산으로 대조한다. `generate_jpeg2000_sampling_inputs.py`는 직접 만든 YUV/알파 원시 샘플을 별도 FFmpeg 네이티브 인코더로32파일 생성하고 C 성분과 독립 색 수식 기대값을 저장한다. 외부 사진·외부 구현 코드를 복사하지 않았다. 전체 ISO/색 관리/실제 외부 앱 전수 검증은 별도 목표다.


순차98: `verify_jpeg2000_origin_outputs.py`는 자체160샘플링 정렬 시작 JP2/3264픽셀의 C 성분·독립 NumPy 상대 좌표/색 수식·정확한 PNG와 SIZ 절대 좌표를 대조한다. 입력/코덱의 전체 적합성 인증은 별도 목표다.


순차99: `verify_jpeg2000_odd_origin_outputs.py`는 자체132비정렬 시작 입력3348픽셀/C 성분·독립 절대 격자/색 수식·정확한 PNG와84빈 색 성분의 C/Rust 읽기 오류를 확인한다. 빈 성분을 이미지로 읽는 기능 및 외부 도구420첫 열 정책 일치는 별도 미완료다.


순차100: `verify_jpeg2000_cmyk_outputs.py`는 자체18CMYK JP2/630픽셀의 C 원시 성분/별도 정수 유리수 변환·COLR/CDEF·정확한 PNG를 대조한다. 프로파일 기반 인쇄 색 관리와 CMYK 네이티브 편집/저장은 별도 미완료다.


순차101: `verify_raw_advanced_outputs.py`가 RAW 통합 보정의 실제 .rphoto v9를 ZIP/JSON으로 읽어 곡선/믹서/그레이딩/디테일/렌즈·32비트·이동/변형/마스크와 합성 DNG의 정확한 원본 바이트/CRC를 확인한다. 기존 RAW 수식 검증은 별도 스크립트로 유지한다.

순차102: `raw_sidebar_tests.rs`와 `raw_preview_tests.rs`는 실제 화면 조작 없이 RAW 좌우/작은 창 전환·설정/원본/취소·맞춤 높이/포인터 좌표·물리 픽셀을 검사한다. 별도 이미지 코덱이나 실제 카메라 검증을 추가한 항목은 아니다.

순차103: `verify_photo_geometry_outputs.py`는 자체 float 사진150경우8640픽셀을 Rust 행렬 계산과 별도 스칼라 역원근/회전·독립3보간으로 비교한다. RAW 레시피/v9 ZIP·32비트/마스크/이동/레이어 변형과 합성 센서의 정확한 원본 바이트를 확인한다. 카메라 기반 자동 기하나 ICC/외부 앱 외관 검증은 아니다.

순차104: `verify_menu_shortcuts.py`는 실제 마이그레이션/저장의 workspace JSON107명령·안정69메뉴 ID와 기존 사용자 키를 독립 확인한다. `menu_shortcut_tests.rs`와 앱 검사는 실제 화면 조작 없이 전체 설정·포인터/키/반복·보호/이력을 확인한다.

순차105–106 수치 레이블은 `numeric_slider`의 실제 headless 포인터/키 입력과 앱 크기 창 적용·취소/원본·한 번 실행 취소로 검사한다. 새로운 외부 이미지나 라이선스 자산은 추가하지 않는다. 단위/정수·소수·로그·정밀 키/작은 이동 누적·포커스/비활성 검사를 포함한다.

순차107 `verify_windows_licenses.py`는 실제 오프라인 Cargo.lock/Windows 일반·빌드 의존성 집합308개를 별도 고정점 탐색으로 비교하고, 포함한528원문을 로컬 정확한 소스 바이트/인코딩/SHA-256과 비교한다. `licenses/windows-dependencies.json`은 수집된 원문 자료이며 전체 배포 요건 완료를 뜻하지 않는다. `qa-output/program-info`의 실제107→109단축키 보존도 독립 검사한다.

순차108 `verify_menu_dispatch.py`는 실제 앱 메뉴89항목/9분류·71안정 ID·현재 키/추가 재실행 키/켜짐 상태/잠금 사유를 `qa-output/menu-dispatch/catalog.json`에서 별도 읽는다. 실제 headless 메뉴 펼침/스크롤·크기/DPI와 클릭/문서 이력은 `menu_app_tests.rs`에서 확인한다.

순차109 `verify_options_dialogs.py`는 실제 옵션 적용에서 저장한 `qa-output/options-dialogs/guides.rphoto`의 v8 ZIP/CRC·4095원래 가이드+추가1/정확한 위치·기본 격자/스냅과 PNG65536흰색 원본 픽셀을 Pillow로 비교한다. 크기/DPI·직접 입력/미리보기/취소/한 번 이력/문서 전환·닫기는 `options_app_tests.rs`의 실제 headless 입력으로 확인한다.

순차110 `verify_color_models.py`는 자체 색 입력/설정 출력 `qa-output/color-models`를 Python 표준 colorsys의 별도 RGB/HSV/HLS 수식으로 비교한다. 3087정확 RGB/알파 왕복·288입력과100반정수 채널(경계1e-9에서만 정수±1 허용), 이전v1 모델 필드 없음/현재HSL 모드·팔레트/최근색/알파를 검사한다. 외부 이미지/라이선스 자산은 추가하지 않는다. 실제 headless 포인터/동기화/작은 창은 `color_model_ui_tests.rs`에서 확인한다.

순차111 `verify_jpeg2000_eycc_outputs.py`는 자체60JP2/2100픽셀의 C OpenJPEG2.5.2 원시 성분과 생성 성분을 정확히 비교하고 별도 NumPy f32 행렬·정수 배율·PNG/COLR24/CDEF·프로젝트 CRC/깊이를 확인한다. 계수/반올림 근거는 OpenJPEG `src/bin/common/color.c`의 `color_esycc_to_rgb`이며 외부 이미지/코드는 복사하지 않았다. 색 깊이5종·부호 크로마4조합·알파3유형을 포함하고 전체 표준 적합성/추가 색 구조/ICC는 별도 목표다.

순차112 `verify_icc_input_outputs.py`는 직접 생성한6 RGB/회색 ICC와30모드4590픽셀의 LittleCMS(Pillow NOOPTIMIZE)8비트 최대1단계·독립 ICC 태그/XYZ 행렬/TRC/sRGB 수식16/32비트 정규 최대0.0007519419(허용0.0008), 정확한 알파·프로젝트 ZIP/CRC/정밀도를 비교한다. ICC 태그 근거는 ICC.1:2010 명세다. 프로파일 생성은 moxcms의 공개 기본 프로파일이고 외부 이미지/코드를 추가하지 않았다. 출력은 `qa-output/icc-input`에 재현하며 검사 후 정리한다. 테스트 기준 입력과 검증 코드는 부산물 삭제 대상이 아니다.

순차113 `verify_jpeg2000_icc_outputs.py`는 자체60JP2/2100픽셀의 C OpenJPEG2.5.2 원시 성분/정규화/알파·method2 ICC 원문, 독립 LittleCMS8비트1단계/ICC 태그·행렬·TRC16비트 정규0.0008이내(실측최대0.00070191501), 정확한 PNG/프로젝트 CRC·깊이를 비교한다.6자체 RGB/회색 프로파일과5색 정밀도·알파2종이다. 공유 QA 수식은 `icc_reference.py`이고 앱 변환 코드는 사용하지 않는다. 출력 `qa-output/jpeg2000-input/icc`는 검사 후 삭제하며 기준 입력/재현 코드는 유지한다.

순차114 `verify_icc_output_files.py`는 자체18PNG/JPEG/TIFF/2754픽셀의 선택한 프로파일 원문·D50 PCS/태그 정렬/길이와 별도 ICC/IFD 추출·포함/제외 파일 간 픽셀 비트·프로젝트 CRC/크기를 비교한다. 배포 프로파일은 테스트 결과가 아닌 `data/color/srgb.icc` 앱 자산이며 기존 로컬 Pillow/LittleCMS 생성 프로파일을 사용하고 출처/고지를 함께 기록했다. 새 프로파일 자료를 다운로드하지 않았다. 생성 출력은 검사 후 정리한다.

순차115 `verify_jpeg2000_icc_output_files.py`는 자체32JP2/4896픽셀의 RGB/GRAY 원문·D50/정렬/CDEF/독립 C 성분·정규화·프로젝트 CRC와16포함/제외 쌍의 정확 codestream/원시 성분 동일을 확인한다. 로컬 GRAY 자산256코드도 LittleCMS로 별도 비교한다. 기준 자산은 data/color에 보존하며 생성 출력은 검사 후 정리한다. 전체 JPEG2000/ICC 적합성 검증은 남음.

순차116 `verify_jpeg2000_metadata_rate_outputs.py`는 자체96출력537696픽셀/24그룹의 정확 codestream·전체 C 성분·288부분 품질층, RGB/GRAY ICC/CDEF·ppi분수·8/16비트 정규화/프로젝트 CRC를 독립 비교한다. 기존 해상도 비교도 포함/제외를 같은 그룹으로 강화했다. 생성 출력은 정리하며 재현 코드/기준 입력은 유지한다.

순차117 `verify_webp_icc_outputs.py`는 자체32WebP3136픽셀의 별도 Pillow/libwebp RGBA/알파·16쌍 VP8/VP8L/ALPH 원문 동일·ICC/RIFF크기/패딩/VP8X치수/정상순서·프로젝트 CRC를 검사한다. 기준 자산/검사 코드를 유지하고 생성 출력은 정리한다. 애니메이션/전체 WebP 적합성 완료를 뜻하지 않는다.

순차118 `verify_source_icc_projects.py`는 자체8PNG/JPEG/TIFF/WebP/JP2 native/P3/잘못된 프로파일 입력의 독립 원문과 프로젝트v12 원문/크기/CRC/플래그를 정확 비교한다. 작업 출력 프로파일과 원본 프로파일을 구분하고 프로파일 없는 v8 구조도 확인한다. 오류 프로젝트와 생성 입력/출력은 검사 후 정리하고 재현 코드/기준 입력은 유지한다.

순차119 `verify_source_metadata_projects.py`는 자체49입력의 PNG chunk CRC/압축 XMP·JPEG APP1·WebP RIFF·TIFF XMP 원문 및 v13 EXIF/XMP/ICC 자산/CRC를 독립 비교한다. Pillow로48입력1680픽셀의8방향/두 바이트 순서·Make/Unicode를 정확 확인한다. 큰 TIFF XMP·이전 v8와 자산 오류도 검사하며 생성 파일은 검사 후 정리한다. 전체 촬영 메타데이터 편집/출력 완료를 뜻하지 않는다.

순차120 `verify_exif_output_files.py`는 자체258출력25542픽셀/129포함·제외 쌍의 정확 압축 데이터·EXIF 방향/IFD0·ExifIFD치수/Make/불투명 원문·독립 픽셀/PNG16·프로젝트 CRC를 검사한다. JPEG 디코더 차이 최대2와 메타데이터 포함의 정확 압축 동일을 구분한다. 16넓은 치수 태그/SHORT→LONG도 별도 Pillow로 읽는다. 생성 출력은 정리하며 재현 코드를 유지한다. 전체 메타데이터/대형 성능 완료를 뜻하지 않는다.
