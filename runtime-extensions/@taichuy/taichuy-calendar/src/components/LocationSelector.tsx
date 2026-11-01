import React, { useState } from 'react';
import { Cascader, Flex } from 'antd';
import pcaCode from 'china-division/dist/pca-code.json';
import coordinatesData from '../data/coordinates';

interface LocationOption {
  code: string;
  name: string;
  children?: LocationOption[];
}

interface LocationSelectorProps {
  value?: string[];
  onChange?: (value: string[], lng: number | null, lat: number | null, labels?: string[]) => void;
}

const LocationSelector: React.FC<LocationSelectorProps> = ({ value, onChange }) => {
  const [internalValue, setInternalValue] = useState<string[]>([]);

  // Use controlled value if provided, otherwise internal state
  const finalValue = value !== undefined ? value : internalValue;

  const getCoordinatesFromCode = (code: string): { lng: number; lat: number } | null => {
    const data = coordinatesData as Record<string, { lat: number; lng: number }>;
    const normalizedCode = code.padEnd(6, '0');

    // 1. Try exact match
    if (data[normalizedCode]) return data[normalizedCode];

    // 2. If it's a district/county, try city level
    if (normalizedCode.substring(4) !== '00') {
      const cityCode = normalizedCode.substring(0, 4) + '00';
      if (data[cityCode]) return data[cityCode];
    }

    // 3. Try province level
    const provinceCode = normalizedCode.substring(0, 2) + '0000';
    if (data[provinceCode]) return data[provinceCode];

    return null;
  };

  const handleLocationChange = (newValue: unknown, selectedOptions: unknown) => {
    const codes = newValue as string[];
    const options = selectedOptions as LocationOption[];
    const labels = options?.map((o) => o.name);

    if (value === undefined) {
      setInternalValue(codes);
    }

    // Try to find coordinates based on the most specific code
    let foundCoords = null;
    if (codes && codes.length > 0) {
      // Try from most specific to least specific
      for (let i = codes.length - 1; i >= 0; i--) {
        const code = codes[i];
        const coords = getCoordinatesFromCode(code);
        if (coords) {
          foundCoords = coords;
          break;
        }
      }
    }

    if (foundCoords) {
      if (onChange) {
        onChange(codes, foundCoords.lng, foundCoords.lat, labels);
      }
    } else {
      if (onChange) {
        onChange(codes, null, null, labels);
      }
    }
  };

  return (
    <Flex vertical style={{ width: '100%' }}>
      <Cascader
        options={pcaCode}
        fieldNames={{ label: 'name', value: 'code', children: 'children' }}
        onChange={handleLocationChange}
        placeholder="请选择出生地点 (省/市/区)"
        style={{ width: '100%' }}
        value={finalValue}
        changeOnSelect
        allowClear
      />
    </Flex>
  );
};

export default LocationSelector;
