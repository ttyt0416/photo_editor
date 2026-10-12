# RGB PSD/PSB 레코드 순서 입력

`../generate_rgb_order_inputs.py`가 psd-tools1.10.9의 별도 바이너리 쓰기로 만든 외부 순서48종과 이전 개발 순서48종이다. 8/16/32비트 × PSD/PSB × 네 가지 압축 × 그룹/독립 레이어를 포함한다. 앞/뒤 레이어의 RGB·α·마스크/클리핑·그룹 순서·태그를 확인하며 원본 수치/두 파일의 SHA-256은 manifest.json에 있다. 합성 입력으로 실제 Photoshop이 만든 파일을 뜻하지 않는다.

폴더는 일반 레이어 헤더를 norm, lsct/8BIM의 별도 블렌드를 pass로 기록해 실제 폴더 설정의 우선순위를 검사한다. [Adobe lsct 규격](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/), [psd-tools 레코드/그룹 구성](https://github.com/psd-tools/psd-tools/blob/main/src/psd_tools/api/psd_image.py)을 대조했다. RGB 합성 이미지의 white matte 표현은 순차49번의 별도 검증 대상이다. ICC는 gray-psd와 동일한 합성 헤더 검사이며 색 변환 기준 프로파일이 아니다.

Rust 검사에서 원본 불변·새 외부 순서 출력·이전 순서/그룹 자동 감지·순서 변경의 명시적 열기·프로젝트·float/HDR·중첩 부모를 확인한다. 독립 `../verify_psd_order_outputs.py`는 새로운48출력과 중첩 그룹2종·기존 압축 그룹24종을 psd-tools로 읽고, 레이어/마스크 및 저장된 합성 평면을 확인한다. 실제 Photoshop 왕복은 별도 미완료 목표다.
